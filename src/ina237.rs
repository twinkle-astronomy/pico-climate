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
const INA237_REG_ENERGY: u8 = 0x08; // Energy register for accumulation
const INA237_REG_CHARGE: u8 = 0x09; // Charge register for accumulation
                                    // const INA237_REG_ALERT_LIMIT: u8 = 0x07;
const INA237_REG_MANUFACTURER_ID: u8 = 0x3E;
const INA237_REG_DEVICE_ID: u8 = 0x3F;

// Configuration register bits
const INA237_CONFIG_RST: u16 = 0x8000;
const INA237_CONFIG_RSTACC: u16 = 0x4000; // Reset accumulation registers
const INA237_CONFIG_ADC_RANGE: u16 = 0x0010;

const INA237_CONVDLY_2MS: u16 = 0x0040;

// ADC configuration bits
const INA237_ADC_256_SAMPLES: u16 = 0x9; // 256 samples averaged
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
    pub energy: f32,        // Energy accumulated since last reset
    pub charge: f32,        // Charge accumulated since last reset
}

impl State {
    pub async fn init_i2c_ina237(&mut self) -> Result<(), Ina237Error> {
        // Check device ID
        let device_id = self.read_register(INA237_REG_DEVICE_ID).await?;
        let manuf_id = self.read_register(INA237_REG_MANUFACTURER_ID).await?;
        // info!("manuf_id: {}", manuf_id);
        // info!("device_id: {}", device_id);
        if manuf_id != 21577 && (device_id != 9072 || device_id != 9089 || device_id != 9104) {
            Timer::after_millis(100).await;
            return Err(Ina237Error::InvalidDeviceId);
        }

        // Reset device and accumulation registers
        self.write_register(INA237_REG_CONFIG, INA237_CONFIG_RST | INA237_CONFIG_RSTACC)
            .await?;
        Timer::after_millis(10).await;

        // Configure device with hardware averaging
        let config = INA237_CONFIG_ADC_RANGE | // ±163.84 mV range
                    INA237_CONVDLY_2MS | // 2ms conversion delay for stability
                    (INA237_ADC_256_SAMPLES << 7) | // Bus voltage ADC: 256 samples averaged
                    (INA237_ADC_256_SAMPLES << 3) | // Shunt voltage ADC: 256 samples averaged
                    INA237_MODE_ALL_TRIG; // Continuous shunt, bus, and temperature

        self.write_register(INA237_REG_CONFIG, config).await?;

        self.calibrate().await?;

        // Initialize the INA state
        // You'll need to add an ina_state field to your State struct
        // self.ina_state = InaState::new();

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

    // Keep original method for compatibility
    pub async fn read_i2c_ina237(&mut self) -> Result<Reading, Ina237Error> {
        info!("READING INA23x");
        let bus_voltage = self.read_bus_voltage().await?;
        let shunt_voltage = self.read_shunt_voltage().await?;
        let current = self.read_current().await?;
        let power = self.read_power().await?;
        let temperature = self.read_die_temperature().await?;
        let energy = self.read_energy().await?;
        let charge = self.read_charge().await?;

        info!("read_bus_voltage: {}", bus_voltage);
        info!("read_shunt_voltage: {}", shunt_voltage);
        info!("read_current: {}", current);
        info!("read_power: {}", power);
        info!("read_die_temperature: {}", temperature);

        Ok(Reading {
            bus_voltage,
            shunt_voltage,
            current,
            power,
            die_temperature: temperature,
            energy,
            charge,
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
        let temperature = (raw_temp as f32) * 0.0078125;
        Ok(temperature)
    }

    pub async fn read_energy(&mut self) -> Result<f32, Ina237Error> {
        let raw_energy = self.read_register(INA237_REG_ENERGY).await?;
        // Energy LSB = 16 × POWER_LSB (in mJ when power is in mW)
        let energy = (raw_energy as f32) * 16.0 * POWER_LSB;
        Ok(energy)
    }

    pub async fn read_charge(&mut self) -> Result<f32, Ina237Error> {
        let raw_charge = self.read_register(INA237_REG_CHARGE).await?;
        // Charge LSB = CURRENT_LSB (in coulombs when current is in amperes)
        let charge = (raw_charge as f32) * CURRENT_LSB;
        Ok(charge)
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
