#![no_std]

use embassy_rp::i2c::Async;
use embassy_rp::peripherals::I2C0;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex as EmbMutex;

pub mod adc_temp_sensor;
pub mod http;
mod ina237;
pub mod prometheus;
mod sht30;
// pub mod tcp_logger;
use defmt_rtt as _;
use static_cell::StaticCell;

pub type Mutex<T> = EmbMutex<CriticalSectionRawMutex, T>;
pub type I2c0 = embassy_rp::i2c::I2c<'static, I2C0, Async>;
pub type I2c0Bus = Mutex<I2c0>;

pub static I2C_BUS_0: StaticCell<I2c0Bus> = StaticCell::new();
