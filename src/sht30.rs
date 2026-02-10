use core::sync::atomic::Ordering;

use defmt::error;
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::Timer;
use embedded_hal::i2c::ErrorType;
use portable_atomic::AtomicF32;

use crate::{I2c0, SampleSet};

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

pub struct Sht30Device<I> {
    addr: u8,
    i2c: I,
}

impl<I: embedded_hal_async::i2c::I2c> Sht30Device<I> {
    pub fn new(i2c: I, addr: u8) -> Self {
        Self { addr, i2c }
    }
    /// Initialize the SHT30 sensor with a soft reset
    pub async fn soft_reset(&mut self) -> Result<(), <I as ErrorType>::Error> {
        self.i2c.write(self.addr, &SHT30_SOFT_RESET).await
    }

    /// Read temperature, humidity, and status from the SHT30 sensor
    pub async fn read(&mut self) -> Result<Reading, <I as ErrorType>::Error> {
        // Clear status register
        self.i2c.write(self.addr, &SHT30_CLEAR_STATUS).await?;

        let mut buffer = [0u8; 6];
        // Trigger measurement (high repeatability with clock stretching)
        self.i2c
            .write_read(self.addr, &SHT30_HIG_REP_CLOCK_STRETCH_READ, &mut buffer)
            .await?;

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
pub async fn continuous_reading(device: &'static mut ContinuousReading) {
    
    if let Err(e) = device.device.soft_reset().await {
        error!("Unable to reset ina237: {:?}", e);
    }

    let mut humidities = SampleSet::<11>::new();
    let mut tempuratures = SampleSet::<11>::new();

    loop {
        Timer::after_millis(100).await;
        match device.device.read().await {
            Ok(reading) => {
                humidities.record(reading.humidity);
                tempuratures.record(reading.temperature);

                if reading.humidity == 0. || reading.temperature == 0. {
                    device.reading.zeros.fetch_add(1., Ordering::Relaxed);
                }
            },
            Err(e) => {
                error!("Error reading sht30: {}", e);
                device.reading.recoverable_errors.fetch_add(1., Ordering::Relaxed);
            }
        }

        device.reading.humidity.store(
            humidities.median(),
            Ordering::Relaxed,
        );

        device.reading.temperature.store(
            tempuratures.median(),
            Ordering::Relaxed,
        );

        // device.reading.recoverable_errors.store(device.device.recoverable_errors as f32, Ordering::Relaxed);
    }
}

pub struct ContinuousReading {
    pub device: Sht30Device<I2cDevice<'static, CriticalSectionRawMutex, I2c0>>,
    pub reading: &'static Output,
}

#[derive(Default)]
pub struct Output {
    pub temperature: AtomicF32,
    pub humidity: AtomicF32,
    pub zeros: AtomicF32,
    pub recoverable_errors: AtomicF32,
    pub heater_status_count: AtomicF32,
    pub humidity_tracking_alert_count: AtomicF32,
    pub temperature_tracking_alert_count: AtomicF32,
    pub command_status_success_count: AtomicF32,
    pub write_data_checksum_status_count: AtomicF32,
}
