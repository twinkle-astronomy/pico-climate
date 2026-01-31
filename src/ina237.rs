use embedded_hal::i2c::ErrorType;

use defmt::{debug, error, info, Format};

use embassy_time::Timer;

// INA237 Register Addresses
const INA237_REG_CONFIG: u8 = 0x00;
const INA237_REG_ADC_CONFIG: u8 = 0x01;
const INA237_REG_SHUNT_CAL: u8 = 0x02;
const INA237_REG_SHUNT_VOLTAGE: u8 = 0x04;
const INA237_REG_BUS_VOLTAGE: u8 = 0x05;
const INA237_REG_DIE_TEMP: u8 = 0x06;
const INA237_REG_CURRENT: u8 = 0x07;
const INA237_REG_POWER: u8 = 0x08;
const INA237_REG_DIAG_ALRT: u8 = 0x0B;
const INA237_REG_SOVL: u8 = 0x0C;
const INA237_REG_SUVL: u8 = 0x0D;
const INA237_REG_BOVL: u8 = 0x0E;
const INA237_REG_BUVL: u8 = 0x0F;
const INA237_REG_TEMP_LIMIT: u8 = 0x10;
const INA237_REG_PWR_LIMIT: u8 = 0x11;
const INA237_REG_MANUFACTURER_ID: u8 = 0x3E;

// CONFIG Register (0x00) Bit Definitions
const INA237_CONFIG_RST: u16 = 1 << 15;
const INA237_CONFIG_CONVDLY_MASK: u16 = 0xFF << 6;
const INA237_CONFIG_ADCRANGE: u16 = 1 << 4;

// ADC_CONFIG Register (0x01) Bit Definitions
const INA237_ADC_CONFIG_MODE_MASK: u16 = 0xF << 12;
const INA237_ADC_CONFIG_VBUSCT_MASK: u16 = 0x7 << 9;
const INA237_ADC_CONFIG_VSHCT_MASK: u16 = 0x7 << 6;
const INA237_ADC_CONFIG_VTCT_MASK: u16 = 0x7 << 3;
const INA237_ADC_CONFIG_AVG_MASK: u16 = 0x7;

// ADC_CONFIG MODE Values (bits 15-12)
const INA237_MODE_SHUTDOWN: u16 = 0x0 << 12;
const INA237_MODE_TRIG_BUS: u16 = 0x1 << 12;
const INA237_MODE_TRIG_SHUNT: u16 = 0x2 << 12;
const INA237_MODE_TRIG_SHUNT_BUS: u16 = 0x3 << 12;
const INA237_MODE_TRIG_TEMP: u16 = 0x4 << 12;
const INA237_MODE_TRIG_TEMP_BUS: u16 = 0x5 << 12;
const INA237_MODE_TRIG_TEMP_SHUNT: u16 = 0x6 << 12;
const INA237_MODE_TRIG_ALL: u16 = 0x7 << 12;
const INA237_MODE_SHUTDOWN2: u16 = 0x8 << 12;
const INA237_MODE_CONT_BUS: u16 = 0x9 << 12;
const INA237_MODE_CONT_SHUNT: u16 = 0xA << 12;
const INA237_MODE_CONT_SHUNT_BUS: u16 = 0xB << 12;
const INA237_MODE_CONT_TEMP: u16 = 0xC << 12;
const INA237_MODE_CONT_TEMP_BUS: u16 = 0xD << 12;
const INA237_MODE_CONT_TEMP_SHUNT: u16 = 0xE << 12;
const INA237_MODE_CONT_ALL: u16 = 0xF << 12;

// VBUSCT - Bus Voltage Conversion Time (bits 11-9)
const INA237_VBUSCT_50US: u16 = 0x0 << 9;
const INA237_VBUSCT_84US: u16 = 0x1 << 9;
const INA237_VBUSCT_150US: u16 = 0x2 << 9;
const INA237_VBUSCT_280US: u16 = 0x3 << 9;
const INA237_VBUSCT_540US: u16 = 0x4 << 9;
const INA237_VBUSCT_1052US: u16 = 0x5 << 9;
const INA237_VBUSCT_2074US: u16 = 0x6 << 9;
const INA237_VBUSCT_4120US: u16 = 0x7 << 9;

// VSHCT - Shunt Voltage Conversion Time (bits 8-6)
const INA237_VSHCT_50US: u16 = 0x0 << 6;
const INA237_VSHCT_84US: u16 = 0x1 << 6;
const INA237_VSHCT_150US: u16 = 0x2 << 6;
const INA237_VSHCT_280US: u16 = 0x3 << 6;
const INA237_VSHCT_540US: u16 = 0x4 << 6;
const INA237_VSHCT_1052US: u16 = 0x5 << 6;
const INA237_VSHCT_2074US: u16 = 0x6 << 6;
const INA237_VSHCT_4120US: u16 = 0x7 << 6;

// VTCT - Temperature Conversion Time (bits 5-3)
const INA237_VTCT_50US: u16 = 0x0 << 3;
const INA237_VTCT_84US: u16 = 0x1 << 3;
const INA237_VTCT_150US: u16 = 0x2 << 3;
const INA237_VTCT_280US: u16 = 0x3 << 3;
const INA237_VTCT_540US: u16 = 0x4 << 3;
const INA237_VTCT_1052US: u16 = 0x5 << 3;
const INA237_VTCT_2074US: u16 = 0x6 << 3;
const INA237_VTCT_4120US: u16 = 0x7 << 3;

// AVG - Averaging Count (bits 2-0)
const INA237_AVG_1: u16 = 0x0;
const INA237_AVG_4: u16 = 0x1;
const INA237_AVG_16: u16 = 0x2;
const INA237_AVG_64: u16 = 0x3;
const INA237_AVG_128: u16 = 0x4;
const INA237_AVG_256: u16 = 0x5;
const INA237_AVG_512: u16 = 0x6;
const INA237_AVG_1024: u16 = 0x7;

// DIAG_ALRT Register (0x0B) Bit Definitions
const INA237_DIAG_ALATCH: u16 = 1 << 15;
const INA237_DIAG_CNVR: u16 = 1 << 14;
const INA237_DIAG_SLOWALERT: u16 = 1 << 13;
const INA237_DIAG_APOL: u16 = 1 << 12;
const INA237_DIAG_MATHOF: u16 = 1 << 9;
const INA237_DIAG_TMPOL: u16 = 1 << 7;
const INA237_DIAG_SHNTOL: u16 = 1 << 6;
const INA237_DIAG_SHNTUL: u16 = 1 << 5;
const INA237_DIAG_BUSOL: u16 = 1 << 4;
const INA237_DIAG_BUSUL: u16 = 1 << 3;
const INA237_DIAG_POL: u16 = 1 << 2;
const INA237_DIAG_CNVRF: u16 = 1 << 1;
const INA237_DIAG_MEMSTAT: u16 = 1 << 0;

// Default I2C address
pub const INA237_DEFAULT_ADDR: u8 = 0x40;

const MAX_EXPECTED_CURRENT: f32 = 100.0;
const CURRENT_LSB: f32 = MAX_EXPECTED_CURRENT / (1 << 15) as f32;
const POWER_LSB: f32 = 3.2 * CURRENT_LSB;

#[derive(Debug, Format)]
pub enum Ina237Error<I: embedded_hal_async::i2c::I2c>
where
    <I as embedded_hal::i2c::ErrorType>::Error: Format,
{
    I2cError(<I as ErrorType>::Error),
    InvalidDeviceId,
}

pub struct Ina237<I> {
    addr: u8,
    i2c: I,
}

impl<I: embedded_hal_async::i2c::I2c> Ina237<I>
where
    <I as embedded_hal::i2c::ErrorType>::Error: Format,
{
    pub fn new(i2c: I, addr: u8) -> Self {
        Self { addr, i2c }
    }

    pub async fn init(&mut self) -> Result<(), Ina237Error<I>> {
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

        if let Err(e) = self.read().await {
            error!("Error reading from ina237: {:?}", e);
        }
        Ok(())
    }

    // Keep original method for compatibility
    pub async fn read(&mut self) -> Result<Reading, Ina237Error<I>> {
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
    }

    pub async fn read_bus_voltage(&mut self) -> Result<f32, Ina237Error<I>> {
        let raw_voltage = self.read_register_i16(INA237_REG_BUS_VOLTAGE).await?;
        // info!("raw_voltage: {}", raw_voltage);
        // Bus voltage LSB = 3.125 mV (with ADC range bit set)
        let voltage = (raw_voltage as f32) * 3.125 / 1000.0;
        Ok(voltage)
    }

    pub async fn read_die_temperature(&mut self) -> Result<f32, Ina237Error<I>> {
        let raw_temp = self.read_register_i16(INA237_REG_DIE_TEMP).await?;

        let raw_temp = raw_temp >> 4;
        // Temperature LSB = : 125 m°C/LSB
        let temperature = (raw_temp as f32) * 125.0 / 1000.;
        Ok(temperature)
    }

    pub async fn read_shunt_voltage(&mut self) -> Result<f32, Ina237Error<I>> {
        let raw_voltage = self.read_register(INA237_REG_SHUNT_VOLTAGE).await? as i16;

        // info!("raw_shunt_voltage: {}", raw_voltage);

        // Return raw value as float
        Ok(raw_voltage as f32)
    }

    pub async fn read_current(&mut self) -> Result<f32, Ina237Error<I>> {
        let raw_current = self.read_register(INA237_REG_CURRENT).await? as i16;
        // Current = raw_value × current_lsb
        let current = (raw_current as f32) * CURRENT_LSB;
        Ok(current)
    }

    pub async fn read_power(&mut self) -> Result<f32, Ina237Error<I>> {
        let raw_power = self.read_register(INA237_REG_POWER).await?;
        // Power = raw_value × power_lsb
        let power = (raw_power as f32) * POWER_LSB;
        Ok(power)
    }

    async fn read_register(&mut self, register: u8) -> Result<u16, Ina237Error<I>> {
        let mut buffer = [0u8; 2];

        self.i2c
            .write_read(self.addr, &[register], &mut buffer)
            .await
            .map_err(Ina237Error::I2cError)?;

        Ok(u16::from_be_bytes(buffer))
    }

    async fn read_register_i16(&mut self, register: u8) -> Result<i16, Ina237Error<I>> {
        let mut buffer = [0u8; 2];

        self.i2c
            .write_read(self.addr, &[register], &mut buffer)
            .await
            .map_err(Ina237Error::I2cError)?;

        Ok(i16::from_be_bytes(buffer))
    }

    async fn write_register(&mut self, register: u8, value: u16) -> Result<(), Ina237Error<I>> {
        let value_bytes = u16::to_be_bytes(value);
        let data = [register, value_bytes[0], value_bytes[1]];
        self.i2c
            .write(self.addr, &data)
            .await
            .map_err(Ina237Error::I2cError)?;
        Ok(())
    }
}

pub struct Reading {
    pub bus_voltage: f32,
    pub shunt_voltage: f32,
    pub current: f32,
    pub power: f32,
    pub die_temperature: f32,
}
