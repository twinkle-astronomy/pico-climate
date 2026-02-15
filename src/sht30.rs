use defmt::{error, info};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Timer};
use embedded_hal::i2c::ErrorType;

use crate::{I2c0, Mutex, SampleSet};

const TICK_TIMEOUT: Duration = Duration::from_millis(1000);

/// Sensor output returned via channel (includes medians and counters)
#[derive(Clone, Copy, Default)]
pub struct Output {
    pub temperature: f32,
    pub humidity: f32,
    pub successes: f32,
    pub timeouts: f32,
    pub zeros: f32,
    pub recoverable_errors: f32,
    pub resets: f32,
    pub heater_status_count: f32,
    pub humidity_tracking_alert_count: f32,
    pub temperature_tracking_alert_count: f32,
    pub command_status_success_count: f32,
    pub write_data_checksum_status_count: f32,
}

pub struct SharedState {
    temperatures: SampleSet<11>,
    humidities: SampleSet<11>,
    successes: f32,
    timeouts: f32,
    zeros: f32,
    recoverable_errors: f32,
    resets: f32,
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
            successes: 0.,
            timeouts: 0.,
            zeros: 0.,
            recoverable_errors: 0.,
            resets: 0.,
            heater_status_count: 0.,
            humidity_tracking_alert_count: 0.,
            temperature_tracking_alert_count: 0.,
            command_status_success_count: 0.,
            write_data_checksum_status_count: 0.,
        }
    }

    pub fn record(&mut self, reading: &Reading) {
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
        self.recoverable_errors += 1.;
    }

    pub fn record_timeout(&mut self) {
        self.timeouts += 1.;
    }

    pub fn record_reset(&mut self) {
        self.resets += 1.;
    }

    pub fn snapshot(&self) -> Output {
        Output {
            temperature: self.temperatures.median(),
            humidity: self.humidities.median(),
            successes: self.successes,
            timeouts: self.timeouts,
            zeros: self.zeros,
            recoverable_errors: self.recoverable_errors,
            resets: self.resets,
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

// SHT30 Commands (no clock stretching)
const SHT30_HIG_REP_NO_STRETCH: [u8; 2] = [0x24, 0x00];
const SHT30_READ_STATUS: [u8; 2] = [0xF3, 0x2D];
const SHT30_CLEAR_STATUS: [u8; 2] = [0x30, 0x41];
const SHT30_SOFT_RESET: [u8; 2] = [0x30, 0xA2];

// Max measurement duration for high repeatability (per datasheet: 15.5ms)
const MEASUREMENT_DELAY: Duration = Duration::from_millis(20);

pub struct Reading {
    pub temperature: f32,
    pub humidity: f32,
    pub heater_status: bool,
    pub humidity_tracking_alert: bool,
    pub temperature_tracking_alert: bool,
    pub command_status_success: bool,
    pub write_data_checksum_status: bool,
}

pub struct Sht30Device<I> {
    addr: u8,
    i2c: I,
}

impl<I: embedded_hal_async::i2c::I2c> Sht30Device<I> {
    pub fn new(i2c: I, addr: u8) -> Self {
        Self { addr, i2c }
    }

    pub async fn soft_reset(&mut self) -> Result<(), <I as ErrorType>::Error> {
        self.i2c.write(self.addr, &SHT30_SOFT_RESET).await
    }

    /// Read temperature, humidity, and status from the SHT30 sensor
    pub async fn read(&mut self) -> Result<Reading, <I as ErrorType>::Error> {
        // Clear status register
        self.i2c.write(self.addr, &SHT30_CLEAR_STATUS).await?;
        Timer::after_millis(1).await;

        // Trigger measurement (high repeatability, no clock stretching)
        self.i2c.write(self.addr, &SHT30_HIG_REP_NO_STRETCH).await?;

        // Wait for measurement to complete
        Timer::after(MEASUREMENT_DELAY).await;

        // Read 6 bytes of measurement data
        let mut buffer = [0u8; 6];
        self.i2c.read(self.addr, &mut buffer).await?;

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
        self.i2c
            .write_read(self.addr, &SHT30_READ_STATUS, &mut buffer)
            .await?;
        Timer::after_millis(1).await;

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
    // return;
    info!("sht30 continuous_reading");
    loop {
        info!("sht30: reset");
        if let Err(e) = embassy_time::with_timeout(TICK_TIMEOUT, device.soft_reset()).await {
            error!("Timeout resetting sht30: {:?}", e);
        }

        Timer::after(Duration::from_secs(5)).await;

        loop {
            // info!("sht30: reading");
            Timer::after(Duration::from_millis(100)).await;
            let result = embassy_time::with_timeout(TICK_TIMEOUT, device.read()).await;

            let mut state = match embassy_time::with_timeout(TICK_TIMEOUT, shared.lock()).await {
                Ok(v) => v,
                Err(_) => {
                    error!("Timeout getting state lock");
                    break;
                }
            };

            match result {
                Ok(Ok(reading)) => {
                    state.record(&reading);
                }
                Ok(Err(e)) => {
                    error!("Error reading sht30: {}", e);
                    state.record_error();
                    state.record_reset();
                    break;
                }
                Err(_) => {
                    error!("Timeout reading sht30, attempting soft reset");
                    state.record_timeout();
                    state.record_reset();
                    break;
                }
            }
            // Timer::after(Duration::from_millis(500)).await;
        }
    }
}
