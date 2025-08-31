#![no_std]

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex as EmbMutex;

pub mod adc_temp_sensor;
pub mod http;
mod ina237;
pub mod prometheus;

pub type Mutex<T> = EmbMutex<CriticalSectionRawMutex, T>;
