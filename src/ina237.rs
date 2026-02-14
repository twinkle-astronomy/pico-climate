use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embedded_hal::i2c::ErrorType;

use defmt::{error, info, Format};

use embassy_time::{Duration, Timer};

use crate::{AverageSet, I2c0, Mutex, SampleSet};

const TICK_TIMEOUT: Duration = Duration::from_millis(1000);

// INA237 Register Addresses
pub const INA237_REG_CONFIG: u8 = 0x00;
pub const INA237_REG_ADC_CONFIG: u8 = 0x01;
pub const INA237_REG_SHUNT_CAL: u8 = 0x02;
pub const INA237_REG_SHUNT_VOLTAGE: u8 = 0x04;
pub const INA237_REG_BUS_VOLTAGE: u8 = 0x05;
pub const INA237_REG_DIE_TEMP: u8 = 0x06;
pub const INA237_REG_CURRENT: u8 = 0x07;
pub const INA237_REG_POWER: u8 = 0x08;
pub const INA237_REG_DIAG_ALRT: u8 = 0x0B;
pub const INA237_REG_SOVL: u8 = 0x0C;
pub const INA237_REG_SUVL: u8 = 0x0D;
pub const INA237_REG_BOVL: u8 = 0x0E;
pub const INA237_REG_BUVL: u8 = 0x0F;
pub const INA237_REG_TEMP_LIMIT: u8 = 0x10;
pub const INA237_REG_PWR_LIMIT: u8 = 0x11;
pub const INA237_REG_MANUFACTURER_ID: u8 = 0x3E;

// CONFIG Register (0x00) Bit Definitions
pub const INA237_CONFIG_RST: u16 = 1 << 15;
pub const INA237_CONFIG_CONVDLY_MASK: u16 = 0xFF << 6;
pub const INA237_CONFIG_ADCRANGE: u16 = 1 << 4;

// ADC_CONFIG Register (0x01) Bit Definitions
pub const INA237_ADC_CONFIG_MODE_MASK: u16 = 0xF << 12;
pub const INA237_ADC_CONFIG_VBUSCT_MASK: u16 = 0x7 << 9;
pub const INA237_ADC_CONFIG_VSHCT_MASK: u16 = 0x7 << 6;
pub const INA237_ADC_CONFIG_VTCT_MASK: u16 = 0x7 << 3;
pub const INA237_ADC_CONFIG_AVG_MASK: u16 = 0x7;

// ADC_CONFIG MODE Values (bits 15-12)
pub const INA237_MODE_SHUTDOWN: u16 = 0x0 << 12;
pub const INA237_MODE_TRIG_BUS: u16 = 0x1 << 12;
pub const INA237_MODE_TRIG_SHUNT: u16 = 0x2 << 12;
pub const INA237_MODE_TRIG_SHUNT_BUS: u16 = 0x3 << 12;
pub const INA237_MODE_TRIG_TEMP: u16 = 0x4 << 12;
pub const INA237_MODE_TRIG_TEMP_BUS: u16 = 0x5 << 12;
pub const INA237_MODE_TRIG_TEMP_SHUNT: u16 = 0x6 << 12;
pub const INA237_MODE_TRIG_ALL: u16 = 0x7 << 12;
pub const INA237_MODE_SHUTDOWN2: u16 = 0x8 << 12;
pub const INA237_MODE_CONT_BUS: u16 = 0x9 << 12;
pub const INA237_MODE_CONT_SHUNT: u16 = 0xA << 12;
pub const INA237_MODE_CONT_SHUNT_BUS: u16 = 0xB << 12;
pub const INA237_MODE_CONT_TEMP: u16 = 0xC << 12;
pub const INA237_MODE_CONT_TEMP_BUS: u16 = 0xD << 12;
pub const INA237_MODE_CONT_TEMP_SHUNT: u16 = 0xE << 12;
pub const INA237_MODE_CONT_ALL: u16 = 0xF << 12;

// VBUSCT - Bus Voltage Conversion Time (bits 11-9)
pub const INA237_VBUSCT_50US: u16 = 0x0 << 9;
pub const INA237_VBUSCT_84US: u16 = 0x1 << 9;
pub const INA237_VBUSCT_150US: u16 = 0x2 << 9;
pub const INA237_VBUSCT_280US: u16 = 0x3 << 9;
pub const INA237_VBUSCT_540US: u16 = 0x4 << 9;
pub const INA237_VBUSCT_1052US: u16 = 0x5 << 9;
pub const INA237_VBUSCT_2074US: u16 = 0x6 << 9;
pub const INA237_VBUSCT_4120US: u16 = 0x7 << 9;

// VSHCT - Shunt Voltage Conversion Time (bits 8-6)
pub const INA237_VSHCT_50US: u16 = 0x0 << 6;
pub const INA237_VSHCT_84US: u16 = 0x1 << 6;
pub const INA237_VSHCT_150US: u16 = 0x2 << 6;
pub const INA237_VSHCT_280US: u16 = 0x3 << 6;
pub const INA237_VSHCT_540US: u16 = 0x4 << 6;
pub const INA237_VSHCT_1052US: u16 = 0x5 << 6;
pub const INA237_VSHCT_2074US: u16 = 0x6 << 6;
pub const INA237_VSHCT_4120US: u16 = 0x7 << 6;

// VTCT - Temperature Conversion Time (bits 5-3)
pub const INA237_VTCT_50US: u16 = 0x0 << 3;
pub const INA237_VTCT_84US: u16 = 0x1 << 3;
pub const INA237_VTCT_150US: u16 = 0x2 << 3;
pub const INA237_VTCT_280US: u16 = 0x3 << 3;
pub const INA237_VTCT_540US: u16 = 0x4 << 3;
pub const INA237_VTCT_1052US: u16 = 0x5 << 3;
pub const INA237_VTCT_2074US: u16 = 0x6 << 3;
pub const INA237_VTCT_4120US: u16 = 0x7 << 3;

// AVG - Averaging Count (bits 2-0)
pub const INA237_AVG_1: u16 = 0x0;
pub const INA237_AVG_4: u16 = 0x1;
pub const INA237_AVG_16: u16 = 0x2;
pub const INA237_AVG_64: u16 = 0x3;
pub const INA237_AVG_128: u16 = 0x4;
pub const INA237_AVG_256: u16 = 0x5;
pub const INA237_AVG_512: u16 = 0x6;
pub const INA237_AVG_1024: u16 = 0x7;

// DIAG_ALRT Register (0x0B) Bit Definitions
pub const INA237_DIAG_ALATCH: u16 = 1 << 15;
pub const INA237_DIAG_CNVR: u16 = 1 << 14;
pub const INA237_DIAG_SLOWALERT: u16 = 1 << 13;
pub const INA237_DIAG_APOL: u16 = 1 << 12;
pub const INA237_DIAG_MATHOF: u16 = 1 << 9;
pub const INA237_DIAG_TMPOL: u16 = 1 << 7;
pub const INA237_DIAG_SHNTOL: u16 = 1 << 6;
pub const INA237_DIAG_SHNTUL: u16 = 1 << 5;
pub const INA237_DIAG_BUSOL: u16 = 1 << 4;
pub const INA237_DIAG_BUSUL: u16 = 1 << 3;
pub const INA237_DIAG_POL: u16 = 1 << 2;
pub const INA237_DIAG_CNVRF: u16 = 1 << 1;
pub const INA237_DIAG_MEMSTAT: u16 = 1 << 0;

// Default I2C address
pub const INA237_DEFAULT_ADDR: u8 = 0x40;

const MAX_EXPECTED_CURRENT: f32 = 100.0;
const CURRENT_LSB: f32 = MAX_EXPECTED_CURRENT / (1 << 15) as f32;
const POWER_LSB: f32 = 3.2 * CURRENT_LSB;

/// Sensor output returned via channel (includes medians and counters)
#[derive(Clone, Copy, Default)]
pub struct Output {
    pub bus_voltage: f32,
    pub shunt_voltage: f32,
    pub current: f32,
    pub successes: f32,
    pub timeouts: f32,
    pub zeros: f32,
    pub recoverable_errors: f32,
    pub resets: f32,
}

pub struct SharedState {
    bus_voltages: SampleSet<11>,
    shunt_voltages: SampleSet<11>,
    currents: AverageSet,
    successes: f32,
    timeouts: f32,
    zeros: f32,
    recoverable_errors: f32,
    resets: f32,
}

impl SharedState {
    pub const fn new() -> Self {
        Self {
            bus_voltages: SampleSet::new(),
            shunt_voltages: SampleSet::new(),
            currents: AverageSet::new(),
            successes: 0.,
            timeouts: 0.,
            zeros: 0.,
            recoverable_errors: 0.,
            resets: 0.,
        }
    }

    pub fn record_bus_voltage(&mut self, v: f32) {
        if v < 10. {
            error!("Voltage read is less than 10v: {}", v);
            self.zeros += 1.;
        } else {
            self.bus_voltages.record(v);
        }
    }

    pub fn record_current(&mut self, v: f32) {
        self.currents.record(v);
    }

    pub fn record_shunt_voltage(&mut self, v: f32) {
        self.shunt_voltages.record(v);
    }

    pub fn set_recoverable_errors(&mut self, count: usize) {
        self.recoverable_errors = count as f32;
    }

    pub fn record_success(&mut self, tick: &TickOutput) {
        self.successes += 1.;
        self.record_bus_voltage(tick.bus_voltage);
        self.record_current(tick.current);
        self.record_shunt_voltage(tick.shunt_voltage);
    }

    pub fn record_timeout(&mut self) {
        self.timeouts += 1.;
    }

    pub fn record_reset(&mut self) {
        self.resets += 1.;
    }

    pub fn snapshot(&mut self) -> Output {
        Output {
            bus_voltage: self.bus_voltages.median(),
            shunt_voltage: self.shunt_voltages.median(),
            current: self.currents.avg(),
            successes: self.successes,
            timeouts: self.timeouts,
            zeros: self.zeros,
            recoverable_errors: self.recoverable_errors,
            resets: self.resets,
        }
    }
}

#[derive(Debug, Format)]
pub enum Ina237Error<I: embedded_hal_async::i2c::I2c>
where
    <I as embedded_hal::i2c::ErrorType>::Error: Format,
{
    I2cError(<I as ErrorType>::Error),
    InvalidDeviceId,
}

pub struct TickOutput {
    pub bus_voltage: f32,
    pub current: f32,
    pub shunt_voltage: f32,
}

pub struct Ina237<I> {
    addr: u8,
    i2c: I,
    recoverable_errors: usize,
}

#[embassy_executor::task]
pub async fn continuous_reading(
    device: &'static mut Ina237<I2cDevice<'static, CriticalSectionRawMutex, I2c0>>,
    shared: &'static Mutex<SharedState>,
) {
    loop {
        if let Err(e) = device.reset().await {
            error!("Unable to reset ina237: {:?}", e);
        }
        if let Err(e) = device.init().await {
            error!("Unable to init ina237: {:?}", e);
        }

        Timer::after_secs(5).await;

        loop {
            let result = embassy_time::with_timeout(TICK_TIMEOUT, device.tick()).await;

            let mut state = match embassy_time::with_timeout(TICK_TIMEOUT, shared.lock()).await {
                Ok(v) => v,
                Err(_) => {
                    error!("Timeout getting state lock");
                    break;
                }
            };

            match result {
                Ok(Ok(output)) => {
                    state.record_success(&output);
                    state.set_recoverable_errors(device.recoverable_errors);
                }
                Ok(Err(e)) => {
                    error!("Error reading ina237: {:?}", e);
                    state.set_recoverable_errors(device.recoverable_errors);
                    state.record_reset();
                    break;
                }
                Err(_) => {
                    state.set_recoverable_errors(device.recoverable_errors);
                    state.record_timeout();
                    state.record_reset();
                    break;
                }
            }
            // Timer::after_millis(100).await;
        }

    }
}

impl<I: embedded_hal_async::i2c::I2c> Ina237<I>
where
    <I as embedded_hal::i2c::ErrorType>::Error: Format,
{
    pub async fn new(i2c: I, addr: u8) -> Result<Self, Ina237Error<I>> {
        let mut dev = Self {
            addr,
            i2c,
            recoverable_errors: 0,
        };

        // Check device ID with timeout
        let manuf_id = match embassy_time::with_timeout(
            embassy_time::Duration::from_millis(1000),
            dev.read_register(INA237_REG_MANUFACTURER_ID),
        )
        .await
        {
            Ok(Ok(id)) => id,
            Ok(Err(e)) => {
                error!("I2C error reading manufacturer ID: {:?}", e);
                return Err(e);
            }
            Err(_) => {
                error!("Timeout reading INA237 - check I2C wiring and pull-up resistors");
                return Err(Ina237Error::InvalidDeviceId);
            }
        };
        if manuf_id != 21577 {
            return Err(Ina237Error::InvalidDeviceId);
        }

        Ok(dev)
    }

    pub async fn reset(&mut self) -> Result<(), Ina237Error<I>> {
        info!("Resetting");
        // Reset device and accumulation registers
        self.write_register(INA237_REG_CONFIG, INA237_CONFIG_RST)
            .await?;
        Timer::after_millis(100).await;
        Ok(())
    }

    pub async fn init(&mut self) -> Result<(), Ina237Error<I>> {
        // let config = INA237_DIAG_ALATCH | INA237_DIAG_CNVR;
        // self.write_register(INA237_REG_DIAG_ALRT, config).await?;


        let config = INA237_MODE_CONT_SHUNT_BUS
            | INA237_VBUSCT_4120US
            | INA237_VSHCT_4120US
            | INA237_VTCT_4120US
            | INA237_AVG_64;
        self.write_register(INA237_REG_ADC_CONFIG, config).await?;

        let calib = (819.2e6 * CURRENT_LSB * 0.015) as u16;
        info!("calib: {}", calib);
        self.write_register(INA237_REG_SHUNT_CAL, calib).await?;
        Timer::after_millis(100).await;

        Ok(())
    }

    /// Perform one full read cycle: trigger conversion, wait for ready, read all registers.
    pub async fn tick(&mut self) -> Result<TickOutput, Ina237Error<I>> {
        // self.trigger().await?;

        self.wait_for_value().await?;
        let bus_voltage = self.read_bus_voltage().await?;
        let current = self.read_current().await?;
        let shunt_voltage = self.read_shunt_voltage().await?;
        Ok(TickOutput {
            bus_voltage,
            current,
            shunt_voltage,
        })
    }

    pub async fn trigger(&mut self) -> Result<(), Ina237Error<I>> {
        let config = INA237_MODE_TRIG_SHUNT_BUS
            | INA237_VBUSCT_4120US
            | INA237_VSHCT_4120US
            | INA237_VTCT_4120US
            | INA237_AVG_1;
        self.write_register(INA237_REG_ADC_CONFIG, config).await?;
        Ok(())
    }

    pub async fn wait_for_value(&mut self) -> Result<(), Ina237Error<I>> {
        loop {
            let diag_alrt = self.read_register(INA237_REG_DIAG_ALRT).await?;

            if diag_alrt & INA237_DIAG_CNVRF != 0 {
                break;
            }
            Timer::after_millis(1).await;
        }
        Ok(())
    }

    pub async fn read_bus_voltage(&mut self) -> Result<f32, Ina237Error<I>> {
        let raw_voltage = self.read_register_i16(INA237_REG_BUS_VOLTAGE).await?;
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

        let mut attempts = 1;
        loop {
            match self
                .i2c
                .write_read(self.addr, &[register], &mut buffer)
                .await
                .map_err(Ina237Error::I2cError)
            {
                Ok(_) => break,
                Err(e) => {
                    if attempts == 3 {
                        return Err(e);
                    }

                    attempts += 1;
                    Timer::after_millis(1).await;
                    self.recoverable_errors += 1;
                    error!("Error reading register {} {:?}", register, e);
                }
            }
        }

        Timer::after_millis(1).await;
        Ok(u16::from_be_bytes(buffer))
    }

    async fn read_register_i16(&mut self, register: u8) -> Result<i16, Ina237Error<I>> {
        let mut buffer = [0u8; 2];

        let mut attempts = 1;
        loop {
            match self
                .i2c
                .write_read(self.addr, &[register], &mut buffer)
                .await
                .map_err(Ina237Error::I2cError)
            {
                Ok(_) => break,
                Err(e) => {
                    if attempts == 3 {
                        return Err(e);
                    }

                    attempts += 1;
                    self.recoverable_errors += 1;
                    Timer::after_millis(1).await;
                    error!("Error reading register {} {:?}", register, e);
                }
            }
        }
        Timer::after_millis(1).await;
        Ok(i16::from_be_bytes(buffer))
    }

    async fn write_register(&mut self, register: u8, value: u16) -> Result<(), Ina237Error<I>> {
        let value_bytes = u16::to_be_bytes(value);
        let data = [register, value_bytes[0], value_bytes[1]];

        let mut attempts = 1;
        loop {
            match self
                .i2c
                .write(self.addr, &data)
                .await
                .map_err(Ina237Error::I2cError)
            {
                Ok(_) => break,
                Err(e) => {
                    if attempts == 3 {
                        return Err(e);
                    }
                    attempts += 1;
                    self.recoverable_errors += 1;
                    Timer::after_millis(1).await;
                    error!("Error writing register {} {:?}", register, e);
                }
            }
        }
        Timer::after_millis(1).await;
        Ok(())
    }
}
