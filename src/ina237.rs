use crate::http::State;
use defmt::{debug, error, info, Format};
use defmt_rtt as _;
use embassy_rp::i2c::Error;
use embassy_time::{with_timeout, Duration, Timer};

// INA237 Register Addresses
const INA237_REG_CONFIG: u8 = 0x00;
const INA237_REG_ADC_CONFIG: u8 = 0x01;
const INA237_REG_SHUNT_CAL: u8 = 0x02;
const INA237_REG_SHUNT_VOLTAGE: u8 = 0x04;
const INA237_REG_BUS_VOLTAGE: u8 = 0x05;
const INA237_REG_DIE_TEMP: u8 = 0x06;
const INA237_REG_CURRENT: u8 = 0x07;
const INA237_REG_POWER: u8 = 0x08;
const INA237_REG_DIAG_ALRT: u8 = 0x0b;

const INA237_REG_MANUFACTURER_ID: u8 = 0x3E;

// Default I2C address
const INA237_DEFAULT_ADDR: u8 = 0x40;

const MAX_EXPECTED_CURRENT: f32 = 100.0;
const CURRENT_LSB: f32 = MAX_EXPECTED_CURRENT / (1 << 15) as f32;
const POWER_LSB: f32 = 3.2 * CURRENT_LSB;

#[derive(Debug, Format)]
pub enum Ina237Error {
    I2cError(Error),
    InvalidDeviceId,
    CalibrationError,
    Timeout,
}

impl From<embassy_time::TimeoutError> for Ina237Error {
    fn from(_: embassy_time::TimeoutError) -> Self {
        Ina237Error::Timeout
    }
}

impl From<Error> for Ina237Error {
    fn from(error: Error) -> Self {
        Ina237Error::I2cError(error)
    }
}

pub struct Reading {
    pub bus_voltage: f32,
    pub shunt_voltage: f32,
    pub current: f32,
    pub power: f32,
    pub die_temperature: f32,
}

impl State {
    pub async fn init_i2c_ina237(&mut self) -> Result<(), Ina237Error> {
        with_timeout(Duration::from_secs(30), async {
            // Check device ID
            let manuf_id = self.read_register(INA237_REG_MANUFACTURER_ID).await?;
            debug!("manuf_id: {}", manuf_id);
            if manuf_id != 21577 {
                return Err(Ina237Error::InvalidDeviceId);
            }

            info!("Resetting");
            // Reset device and accumulation registers
            self.write_register(INA237_REG_CONFIG, 1 << 15).await?;
            Timer::after_millis(100).await;

            let config: u16 = 0b0_0_00000010_0_0_0000;
            info!("config: {}", config);
            self.write_register(INA237_REG_CONFIG, config).await?;
            Timer::after_millis(100).await;

            let calib = (819.2e6 * CURRENT_LSB * 0.015) as u16;
            info!("calib: {}", calib);
            self.write_register(INA237_REG_SHUNT_CAL, calib).await?;
            Timer::after_millis(100).await;

            if let Err(e) = self.read_i2c_ina237().await {
                error!("Error reading from ina237: {:?}", e);
            }
            Ok(())
        }).await?
    }

    // Keep original method for compatibility
    pub async fn read_i2c_ina237(&mut self) -> Result<Reading, Ina237Error> {
        with_timeout(Duration::from_secs(5), async {
            // info!("READING INA23x");
            let config: u16 = 0b0111_000_000_000_010;
            self.write_register(INA237_REG_ADC_CONFIG, config).await?;

            loop {
                let diag_alrt = self.read_register(INA237_REG_DIAG_ALRT).await?;

                if diag_alrt & 0b10 != 0 {
                    break;
                }
                Timer::after_millis(10).await;
            }
            Timer::after_millis(100).await;

            let die_temperature = self.read_die_temperature().await?;
            let bus_voltage = self.read_bus_voltage().await?;
            let shunt_voltage = self.read_shunt_voltage().await?;
            let current = self.read_current().await?;
            let power = 0.; //self.read_power().await?;

            // info!("read_bus_voltage: {}", bus_voltage);
            // info!("read_shunt_voltage: {}", shunt_voltage);
            // info!("read_current: {}", current);
            // info!("read_power: {}", power);
            // info!("read_die_temperature: {}", die_temperature);

            Ok(Reading {
                bus_voltage,
                shunt_voltage,
                current,
                power,
                die_temperature,
            })
        })
        .await?
    }

    pub async fn read_bus_voltage(&mut self) -> Result<f32, Ina237Error> {
        with_timeout(Duration::from_secs(1), async {
            let raw_voltage = self.read_register_i16(INA237_REG_BUS_VOLTAGE).await?;
            // info!("raw_voltage: {}", raw_voltage);
            // Bus voltage LSB = 3.125 mV (with ADC range bit set)
            let voltage = (raw_voltage as f32) * 3.125 / 1000.0;
            Ok(voltage)
        })
        .await?
    }

    pub async fn read_die_temperature(&mut self) -> Result<f32, Ina237Error> {
        with_timeout(Duration::from_secs(1), async {
            let raw_temp = self.read_register_i16(INA237_REG_DIE_TEMP).await?;

            let raw_temp = raw_temp >> 4;
            // Temperature LSB = : 125 m°C/LSB
            let temperature = (raw_temp as f32) * 125.0 / 1000.;
            Ok(temperature)
        })
        .await?
    }

    pub async fn read_shunt_voltage(&mut self) -> Result<f32, Ina237Error> {
        with_timeout(Duration::from_secs(1), async {
            let raw_voltage = self.read_register(INA237_REG_SHUNT_VOLTAGE).await? as i16;

            // info!("raw_shunt_voltage: {}", raw_voltage);

            // Return raw value as float
            Ok(raw_voltage as f32)
        })
        .await?
    }

    pub async fn read_current(&mut self) -> Result<f32, Ina237Error> {
        with_timeout(Duration::from_secs(1), async {
            let raw_current = self.read_register(INA237_REG_CURRENT).await? as i16;
            // Current = raw_value × current_lsb
            let current = (raw_current as f32) * CURRENT_LSB;
            Ok(current)
        })
        .await?
    }

    pub async fn read_power(&mut self) -> Result<f32, Ina237Error> {
        with_timeout(Duration::from_secs(1), async {
            let raw_power = self.read_register(INA237_REG_POWER).await?;
            // Power = raw_value × power_lsb
            let power = (raw_power as f32) * POWER_LSB;
            Ok(power)
        })
        .await?
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

        Ok(u16::from_be_bytes(buffer))
    }

    async fn read_register_i16(&mut self, register: u8) -> Result<i16, Ina237Error> {
        let mut buffer = [0u8; 2];

        // Write register address
        self.i2c
            .write_async(INA237_DEFAULT_ADDR, [register].into_iter())
            .await?;

        // Read register value
        self.i2c
            .read_async(INA237_DEFAULT_ADDR, &mut buffer)
            .await?;

        Ok(i16::from_be_bytes(buffer))
    }

    async fn write_register(&mut self, register: u8, value: u16) -> Result<(), Error> {
        let data = [register]
            .into_iter()
            .chain(u16::to_be_bytes(value).into_iter());
        self.i2c.write_async(INA237_DEFAULT_ADDR, data).await
    }
}
