use crate::http::State;
use defmt::{error, info, Format};
use defmt_rtt as _;
use embassy_rp::i2c::Error;
use embassy_time::Timer;

// INA237 Register Addresses
const INA237_REG_CONFIG: u8 = 0x00;
const INA237_REG_SHUNT_VOLTAGE: u8 = 0x01;
const INA237_REG_BUS_VOLTAGE: u8 = 0x02;
const INA237_REG_POWER: u8 = 0x03;
const INA237_REG_CURRENT: u8 = 0x04;
const INA237_REG_CALIBRATION: u8 = 0x05;
const INA237_REG_DIE_TEMP: u8 = 0x06;
// const INA237_REG_ALERT_LIMIT: u8 = 0x07;
const INA237_REG_MANUFACTURER_ID: u8 = 0x3E;
const INA237_REG_DEVICE_ID: u8 = 0x3F;

// Configuration register bits
const INA237_CONFIG_RST: u16 = 0x8000;
// const INA237_CONFIG_RSTACC: u16 = 0x4000;
// const INA237_CONFIG_CONVDLY: u16 = 0x03C0;
const INA237_CONFIG_ADC_RANGE: u16 = 0x0010;

// ADC configuration bits
// const INA237_CONFIG_BADC_MASK: u16 = 0x0780;
// const INA237_CONFIG_SADC_MASK: u16 = 0x0078;
// const INA237_CONFIG_MODE_MASK: u16 = 0x0007;

// ADC configuration values
// const INA237_ADC_50US: u16 = 0x0;
// const INA237_ADC_84US: u16 = 0x1;
// const INA237_ADC_150US: u16 = 0x2;
// const INA237_ADC_280US: u16 = 0x3;
// const INA237_ADC_540US: u16 = 0x4;
const INA237_ADC_1052US: u16 = 0x5;
// const INA237_ADC_2074US: u16 = 0x6;
// const INA237_ADC_4120US: u16 = 0x7;

// Operating mode values
// const INA237_MODE_SHUTDOWN: u16 = 0x0;
// const INA237_MODE_SHUNT_TRIG: u16 = 0x1;
// const INA237_MODE_BUS_TRIG: u16 = 0x2;
// const INA237_MODE_SHUNT_BUS_TRIG: u16 = 0x3;
// const INA237_MODE_TEMP_TRIG: u16 = 0x4;
// const INA237_MODE_SHUNT_TEMP_TRIG: u16 = 0x5;
// const INA237_MODE_BUS_TEMP_TRIG: u16 = 0x6;
const INA237_MODE_ALL_TRIG: u16 = 0x7;

// Default I2C address
const INA237_DEFAULT_ADDR: u8 = 0x40;

#[derive(Debug, Format)]
pub enum Ina237Error {
    I2cError(Error),
    InvalidDeviceId,
    CalibrationError,
}

impl From<Error> for Ina237Error {
    fn from(error: Error) -> Self {
        Ina237Error::I2cError(error)
    }
}

const MAX_EXPECTED_CURRENT: f32 = 10.0; // Amperes
const CURRENT_LSB: f32 = MAX_EXPECTED_CURRENT / 32768.0;
const POWER_LSB: f32 = 3.2 * CURRENT_LSB;

pub struct Reading {
    pub bus_voltage: f32,
    pub shunt_voltage: f32,
    pub current: f32,
    pub power: f32,
    pub die_temperature: f32,
}

impl State {
    pub async fn init_i2c_ina237(&mut self) -> Result<(), Ina237Error> {
        // Check device ID
        let device_id = self.read_register(INA237_REG_DEVICE_ID).await?;
        let manuf_id = self.read_register(INA237_REG_MANUFACTURER_ID).await?;
        info!("manuf_id: {}", manuf_id);
        info!("device_id: {}", device_id);
        if manuf_id != 21577 && (device_id != 9072 || device_id != 9089 || device_id != 9104) {
            Timer::after_millis(100).await;
            return Err(Ina237Error::InvalidDeviceId);
        }

        // Reset device
        self.write_register(INA237_REG_CONFIG, INA237_CONFIG_RST)
            .await?;
        Timer::after_millis(10).await;

        // Configure device
        let config = INA237_CONFIG_ADC_RANGE | // ±163.84 mV range
                    (INA237_ADC_1052US << 7) | // Bus voltage ADC: 1052μs
                    (INA237_ADC_1052US << 3) | // Shunt voltage ADC: 1052μs
                    INA237_MODE_ALL_TRIG; // Continuous shunt, bus, and temperature

        self.write_register(INA237_REG_CONFIG, config).await?;

        self.calibrate().await?;

        // self.i2c.write_async(addr, bytes)
        if let Err(e) = self.read_i2c_ina237().await {
            error!("Error reading from ina237: {:?}", e);
        }

        Ok(())
    }
    async fn calibrate(&mut self) -> Result<(), Ina237Error> {
        // Calculate calibration register value
        // CAL = 13107.2 × 10^6 × CURRENT_LSB × R_SHUNT
        let shunt_resistance = 0.1;
        let cal_value = (13107200.0 * CURRENT_LSB * shunt_resistance) as u16;

        if cal_value == 0 {
            return Err(Ina237Error::CalibrationError);
        }

        // Write calibration register
        self.write_register(INA237_REG_CALIBRATION, cal_value)
            .await?;

        Ok(())
    }
    pub async fn read_i2c_ina237(&mut self) -> Result<Reading, Ina237Error> {
        info!("READING INA23x");
        info!("read_bus_voltage: {}", self.read_bus_voltage().await);
        info!("read_shunt_voltage: {}", self.read_shunt_voltage().await);
        info!("read_current: {}", self.read_current().await);
        info!("read_power: {}", self.read_power().await);
        info!(
            "read_die_temperature: {}",
            self.read_die_temperature().await
        );
        Ok(Reading {
            bus_voltage: self.read_bus_voltage().await?,
            shunt_voltage: self.read_shunt_voltage().await?,
            current: self.read_current().await?,
            die_temperature: self.read_die_temperature().await?,
            power: self.read_power().await?,
        })
    }

    pub async fn read_bus_voltage(&mut self) -> Result<f32, Ina237Error> {
        let raw_voltage = self.read_register(INA237_REG_BUS_VOLTAGE).await?;

        // Bus voltage LSB = 3.125 mV (with ADC range bit set)
        let voltage = (raw_voltage as f32) * 0.003125;

        Ok(voltage)
    }

    pub async fn read_shunt_voltage(&mut self) -> Result<f32, Ina237Error> {
        let raw_voltage = self.read_register(INA237_REG_SHUNT_VOLTAGE).await? as i16;

        // Shunt voltage LSB = 5 μV (with ADC range bit set)
        let voltage = (raw_voltage as f32) * 0.000005;

        Ok(voltage)
    }

    pub async fn read_current(&mut self) -> Result<f32, Ina237Error> {
        let raw_current = self.read_register(INA237_REG_CURRENT).await? as i16;

        // Current = raw_value × current_lsb
        let current = (raw_current as f32) * CURRENT_LSB;

        Ok(current)
    }

    pub async fn read_power(&mut self) -> Result<f32, Ina237Error> {
        let raw_power = self.read_register(INA237_REG_POWER).await?;

        // Power = raw_value × power_lsb
        let power = (raw_power as f32) * POWER_LSB;

        Ok(power)
    }

    pub async fn read_die_temperature(&mut self) -> Result<f32, Ina237Error> {
        let raw_temp = self.read_register(INA237_REG_DIE_TEMP).await? as i16;

        // Temperature LSB = 7.8125 m°C (0.0078125°C)
        // Formula: Temperature = raw_value × 7.8125 m°C
        let temperature = (raw_temp as f32) * 0.0078125;

        Ok(temperature)
    }

    async fn read_register(&mut self, register: u8) -> Result<u16, Ina237Error> {
        let mut buffer = [0u8; 2];

        // Write register address
        self.i2c
            .write_async(INA237_DEFAULT_ADDR, [register].into_iter())
            .await?;

        // Read register value
        self.i2c
            .read_async(INA237_DEFAULT_ADDR, &mut buffer)
            .await?;

        Ok(((buffer[0] as u16) << 8) | (buffer[1] as u16))
    }

    async fn write_register(&mut self, register: u8, value: u16) -> Result<(), Error> {
        let data = [register, (value >> 8) as u8, (value & 0xFF) as u8];
        self.i2c.write_async(INA237_DEFAULT_ADDR, data).await
    }
}
