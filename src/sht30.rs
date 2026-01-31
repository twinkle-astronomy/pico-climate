use embedded_hal::i2c::ErrorType;

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
