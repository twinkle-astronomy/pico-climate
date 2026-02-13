#![no_std]

use embassy_rp::i2c::Async;
use embassy_rp::peripherals::I2C0;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex as EmbMutex;

pub mod adc_temp_sensor;
pub mod http;
pub mod ina237;
pub mod prometheus;
pub mod sht30;
// pub mod tcp_logger;
use defmt_rtt as _;
use heapless::Vec;
use static_cell::StaticCell;

pub type Mutex<T> = EmbMutex<CriticalSectionRawMutex, T>;

pub type I2c0 = embassy_rp::i2c::I2c<'static, I2C0, Async>;
pub type I2c0Bus = Mutex<I2c0>;
pub static I2C_BUS_0: StaticCell<I2c0Bus> = StaticCell::new();

pub struct AverageSet {
    sum: f32,
    count: usize,
}

impl AverageSet {
    pub const fn new() -> Self {
        Self { sum: 0., count: 0 }
    }

    pub fn record(&mut self, sample: f32) {
        self.sum += sample;
        self.count += 1;
    }

    pub fn avg(&mut self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }

        let avg = self.sum / self.count as f32;
        self.count = 0;
        self.sum = 0.;
        avg
    }
}

pub struct SampleSet<const N: usize> {
    samples: [f32; N],
    count: usize,
}

impl<const N: usize> SampleSet<N> {
    pub const fn new() -> Self {
        Self {
            samples: [0.; N],
            count: 0,
        }
    }

    pub fn record(&mut self, sample: f32) {
        self.samples[self.count % N] = sample;
        self.count += 1;
    }

    pub fn median(&self) -> f32 {
        let sample_count = self.sample_count();

        let mut samples = self
            .samples
            .iter()
            .take(sample_count)
            .collect::<Vec<&f32, N>>();
        samples.sort_unstable_by(|a, b| a.total_cmp(b));

        *samples[samples.len() / 2]
    }

    fn sample_count(&self) -> usize {
        if self.count > N {
            N
        } else {
            self.count
        }
    }
}
