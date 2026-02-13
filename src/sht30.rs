use defmt::{error, Format};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Timer};
use embedded_hal::i2c::ErrorType;

use crate::{I2c0, Mutex, SampleSet};

const I2C_TIMEOUT: Duration = Duration::from_millis(100);

/// Sensor output returned via channel (includes medians and counters)
#[derive(Clone, Copy, Default)]
pub struct Output {
    pub temperature: f32,
    pub humidity: f32,
    pub reads: f32,
    pub successes: f32,
    pub timeouts: f32,
    pub zeros: f32,
    pub recoverable_errors: f32,
    pub heater_status_count: f32,
    pub humidity_tracking_alert_count: f32,
    pub temperature_tracking_alert_count: f32,
    pub command_status_success_count: f32,
    pub write_data_checksum_status_count: f32,
}

pub struct SharedState {
    temperatures: SampleSet<11>,
    humidities: SampleSet<11>,
    reads: f32,
    successes: f32,
    timeouts: f32,
    zeros: f32,
    recoverable_errors: f32,
    heater_status_count: f32,
    humidity_tracking_alert_count: f32,
    temperature_tracking_alert_count: f32,
    command_status_success_count: f32,
    write_data_checksum_status_count: f32,
}

impl SharedState {
    pub const fn new() -> Self {
        Self {
            temperatures: SampleSet::new(),
            humidities: SampleSet::new(),
            reads: 0.,
            successes: 0.,
            timeouts: 0.,
            zeros: 0.,
            recoverable_errors: 0.,
            heater_status_count: 0.,
            humidity_tracking_alert_count: 0.,
            temperature_tracking_alert_count: 0.,
            command_status_success_count: 0.,
            write_data_checksum_status_count: 0.,
        }
    }

    pub fn record(&mut self, reading: &Reading) {
        self.reads += 1.;
        self.successes += 1.;
        self.humidities.record(reading.humidity);
        self.temperatures.record(reading.temperature);

        if reading.humidity == 0. || reading.temperature == 0. {
            self.zeros += 1.;
        }
        if reading.heater_status {
            self.heater_status_count += 1.;
        }
        if reading.humidity_tracking_alert {
            self.humidity_tracking_alert_count += 1.;
        }
        if reading.temperature_tracking_alert {
            self.temperature_tracking_alert_count += 1.;
        }
        if reading.command_status_success {
            self.command_status_success_count += 1.;
        }
        if reading.write_data_checksum_status {
            self.write_data_checksum_status_count += 1.;
        }
    }

    pub fn record_error(&mut self) {
        self.reads += 1.;
        self.recoverable_errors += 1.;
    }

    pub fn record_timeout(&mut self) {
        self.reads += 1.;
        self.timeouts += 1.;
    }

    pub fn snapshot(&self) -> Output {
        Output {
            temperature: self.temperatures.median(),
            humidity: self.humidities.median(),
            reads: self.reads,
            successes: self.successes,
            timeouts: self.timeouts,
            zeros: self.zeros,
            recoverable_errors: self.recoverable_errors,
            heater_status_count: self.heater_status_count,
            humidity_tracking_alert_count: self.humidity_tracking_alert_count,
            temperature_tracking_alert_count: self.temperature_tracking_alert_count,
            command_status_success_count: self.command_status_success_count,
            write_data_checksum_status_count: self.write_data_checksum_status_count,
        }
    }
}

// SHT30 I2C Address
pub const SHT30_ADDR: u8 = 0x44;

// SHT30 Commands
const SHT30_HIG_REP_CLOCK_STRETCH_READ: [u8; 2] = [0x2C, 0x06];
const SHT30_READ_STATUS: [u8; 2] = [0xF3, 0x2D];
const SHT30_CLEAR_STATUS: [u8; 2] = [0x30, 0x41];
const SHT30_SOFT_RESET: [u8; 2] = [0x30, 0xA2];

pub struct Reading {
    pub temperature: f32,
    pub humidity: f32,
    pub heater_status: bool,
    pub humidity_tracking_alert: bool,
    pub temperature_tracking_alert: bool,
    pub command_status_success: bool,
    pub write_data_checksum_status: bool,
}

#[derive(Format)]
pub enum Sht30Error<E: Format> {
    I2c(E),
    Timeout,
}

pub struct Sht30Device<I> {
    addr: u8,
    i2c: I,
}

impl<I: embedded_hal_async::i2c::I2c> Sht30Device<I>
where
    <I as ErrorType>::Error: Format,
{
    pub fn new(i2c: I, addr: u8) -> Self {
        Self { addr, i2c }
    }
    /// Initialize the SHT30 sensor with a soft reset
    pub async fn soft_reset(&mut self) -> Result<(), Sht30Error<<I as ErrorType>::Error>> {
        embassy_time::with_timeout(I2C_TIMEOUT, self.i2c.write(self.addr, &SHT30_SOFT_RESET))
            .await
            .map_err(|_| Sht30Error::Timeout)?
            .map_err(Sht30Error::I2c)
    }

    /// Read temperature, humidity, and status from the SHT30 sensor
    pub async fn read(&mut self) -> Result<Reading, Sht30Error<<I as ErrorType>::Error>> {
        // Clear status register
        embassy_time::with_timeout(I2C_TIMEOUT, self.i2c.write(self.addr, &SHT30_CLEAR_STATUS))
            .await
            .map_err(|_| Sht30Error::Timeout)?
            .map_err(Sht30Error::I2c)?;

        let mut buffer = [0u8; 6];
        // Trigger measurement (high repeatability with clock stretching)
        embassy_time::with_timeout(
            I2C_TIMEOUT,
            self.i2c
                .write_read(self.addr, &SHT30_HIG_REP_CLOCK_STRETCH_READ, &mut buffer),
        )
        .await
        .map_err(|_| Sht30Error::Timeout)?
        .map_err(Sht30Error::I2c)?;

        // Parse temperature data (first 3 bytes)
        let temp_raw = ((buffer[0] as u16) << 8) | (buffer[1] as u16);
        // Note: buffer[2] is CRC - skipped for simplicity

        // Parse humidity data (next 3 bytes)
        let hum_raw = ((buffer[3] as u16) << 8) | (buffer[4] as u16);
        // Note: buffer[5] is CRC - skipped for simplicity

        // Convert to actual values using SHT30 formulas
        let temperature = -45.0 + 175.0 * (temp_raw as f32) / 65535.0;
        let humidity = 100.0 * (hum_raw as f32) / 65535.0;

        // Read status register
        let mut buffer = [0u8; 2];
        embassy_time::with_timeout(
            I2C_TIMEOUT,
            self.i2c
                .write_read(self.addr, &SHT30_READ_STATUS, &mut buffer),
        )
        .await
        .map_err(|_| Sht30Error::Timeout)?
        .map_err(Sht30Error::I2c)?;

        let status: u16 = ((buffer[0] as u16) << 8) | (buffer[1] as u16);

        // Parse status bits
        let heater_status = status & 0b0100_0000_0000_0000 != 0;
        let humidity_tracking_alert = status & 0b0001_0000_0000_0000 != 0;
        let temperature_tracking_alert = status & 0b0000_1000_0000_0000 != 0;
        let command_status_success = status & 0b0000_0000_0000_0010 != 0;
        let write_data_checksum_status = status & 0b0000_0000_0000_0001 != 0;

        Ok(Reading {
            temperature,
            humidity,
            heater_status,
            humidity_tracking_alert,
            temperature_tracking_alert,
            command_status_success,
            write_data_checksum_status,
        })
    }
}

#[embassy_executor::task]
pub async fn continuous_reading(
    device: &'static mut Sht30Device<I2cDevice<'static, CriticalSectionRawMutex, I2c0>>,
    shared: &'static Mutex<SharedState>,
) {
    if let Err(e) = device.soft_reset().await {
        error!("Unable to reset sht30: {:?}", e);
    }

    loop {
        match device.read().await {
            Ok(reading) => {
                shared.lock().await.record(&reading);
            }
            Err(Sht30Error::Timeout) => {
                error!("Timeout reading sht30, attempting soft reset");
                shared.lock().await.record_timeout();
                if let Err(e) = device.soft_reset().await {
                    error!("Unable to reset sht30 after timeout: {:?}", e);
                }
            }
            Err(e) => {
                error!("Error reading sht30: {:?}", e);
                shared.lock().await.record_error();
            }
        }
        Timer::after_millis(100).await;
    }
}
