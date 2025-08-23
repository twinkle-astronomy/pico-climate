use embassy_rp::adc::{Adc, Async, Channel, Error};

pub struct Sensor<'a> {
    pub adc: Adc<'a, Async>,
    pub temp_sensor: Channel<'a>,
}

pub struct Value {
    pub temp_celsius: f32,
    pub volt: f32,
    pub raw: u16,
}

impl<'a> Sensor<'a> {
    pub async fn read(&mut self) -> Result<Value, Error> {
        let raw = self.adc.read(&mut self.temp_sensor).await?;

        // Convert to temperature in Celsius
        // RP2040 datasheet formula: T = 27 - (ADC_voltage - 0.706)/0.001721
        let volt = (raw as f32 * 3.29) / 4096.0; // 12-bit ADC, 3.3V reference
        let temp_celsius = 27. - (volt - 0.706) / 0.001721;

        Ok(Value {
            temp_celsius,
            volt,
            raw,
        })
    }
}
