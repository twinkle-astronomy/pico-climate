use core::ops::Deref;

use defmt::{error, info};
use embassy_net::Stack;
use embassy_rp::i2c::{Async, I2c};
use embassy_rp::peripherals::I2C0;
use embassy_time::{Duration, Instant};
use picoserve::response::chunked::ChunkedResponse;
use picoserve::response::IntoResponse;
use picoserve::routing::get;

use defmt_rtt as _;
use static_cell::StaticCell;

use crate::prometheus::{counter, gauge, Histogram, MetricsResponse, Sample, WriteMetric};
use crate::{adc_temp_sensor, Mutex};

pub static LAST_REQUEST_TIME: Mutex<Instant> = Mutex::new(Instant::MIN);

pub static WIFI_RSSI_STRENGTH: Mutex<Histogram<10>> = Mutex::new(Histogram::new([10., 20., 30., 40., 50., 60., 70.,80.,90.,100.]));

const SHT30_ADDR: u16 = 0x44;
const SHT30_HIG_REP_CLOCK_STRETCH_READ: [u8; 2] = [0x2C, 0x06];
const SHT30_READ_STATUS: [u8; 2] = [0xF3, 0x2D];
const SHT30_CLEAR_STATUS: [u8; 2] = [0x30, 0x41];

struct PicoClimateMetrics {
    app_state: AppState,
}

impl WriteMetric for PicoClimateMetrics {
    async fn write_chunks<W>(
        &self,
        chunk_writer: &mut picoserve::response::chunked::ChunkWriter<W>,
        
    ) -> Result<(), W::Error>
    where
        W: picoserve::io::Write,
    {
        let mut app_state_lock = self.app_state.state.lock().await;
        app_state_lock.count = app_state_lock.count + 1;

        counter(
            "http_request_count",
            "Number of http requests recieved",
            [],
            [Sample::new([], app_state_lock.count as f32)],
        ).write_chunks(chunk_writer).await?;


        if let Ok(adc_sample) = app_state_lock.adc_temp_sensor.read().await {
            gauge(
                "adc_temp_sensor",
                "Value of onboard temp sensor",
                ["unit"],
                [
                    Sample::new(["C"], adc_sample.temp_celsius),
                    Sample::new(["volts"], adc_sample.volt),
                    Sample::new(["raw"], adc_sample.raw as f32),
                ],
            ).write_chunks(chunk_writer).await?;
        }

        match app_state_lock.read_i2c_sht30().await {
            Ok(I2CReading {
                temperature,
                humidity,
                heater_status,
                humidity_tracking_alert,
                temperature_tracking_alert,
                command_status_success,
                write_data_checksum_status,
            }) => {
                gauge(
                    "sht30_reading",
                    "Reading from SHT30 Sensor",
                    ["sensor"],
                    [
                        Sample::new(["temperature"], temperature),
                        Sample::new(["humidity"], humidity),
                    ],
                ).write_chunks(chunk_writer).await?;
                gauge(
                    "sht30_status",
                    "SHT30 Status Registers",
                    ["feature"],
                    [
                        Sample::new(["heater_status"], if heater_status { 1. } else { 0. }),
                        Sample::new(
                            ["humidity_tracking_alert"],
                            if humidity_tracking_alert { 1. } else { 0. },
                        ),
                        Sample::new(
                            ["temperature_tracking_alert"],
                            if temperature_tracking_alert { 1. } else { 0. },
                        ),
                        Sample::new(
                            ["command_status_success"],
                            if command_status_success { 1. } else { 0. },
                        ),
                        Sample::new(
                            ["write_data_checksum_status"],
                            if write_data_checksum_status { 1. } else { 0. },
                        ),
                    ],
                ).write_chunks(chunk_writer).await?;
            }
            Err(e) => {
                error!("Got error reading i2c: {:?}", e);
                app_state_lock.sht30_errors += 1;
            }
        };
        counter(
            "sht30_error",
            "Errors reading from SHT30 Sensor",
            [],
            [Sample::new([], app_state_lock.sht30_errors as f32)],
        ).write_chunks(chunk_writer).await?;

            
        if app_state_lock.has_ina237 {
            if let Ok(reading) = app_state_lock.read_i2c_ina237().await {
                gauge(
                    "ina237_reading",
                    "register values from INA237 Sensor",
                    ["register"],
                    [
                        Sample::new(["bus_voltage"], reading.bus_voltage),
                        Sample::new(["shunt_voltage"], reading.shunt_voltage),
                        Sample::new(["current"], reading.current),
                        Sample::new(["power"], reading.power),
                        Sample::new(["die_temperature"], reading.die_temperature),
                    ],
                ).write_chunks(chunk_writer).await?;
            }
        }

        Ok(())
    }
}

async fn metrics(
    picoserve::extract::State(app_state): picoserve::extract::State<AppState>,
) -> impl IntoResponse {
    info!("GET /metrics");
    {
        let mut last_req = LAST_REQUEST_TIME.lock().await;
        *last_req = Instant::now();
    }

    ChunkedResponse::new(MetricsResponse::new(PicoClimateMetrics {
        app_state
    }))
}

#[derive(Clone, Copy)]
pub struct AppState {
    state: &'static Mutex<State>,
}

impl AppState {
    pub async fn new(
        adc_temp_sensor: &'static mut adc_temp_sensor::Sensor<'static>,
        mut i2c: I2c<'static, I2C0, Async>,
    ) -> Result<Self, embassy_rp::i2c::Error> {
        i2c.write_async(SHT30_ADDR, [0x30, 0xA2]).await?;

        static STATE: StaticCell<Mutex<State>> = StaticCell::new();
        let state = STATE.init(Mutex::new(State {
            count: 0,
            adc_temp_sensor,
            sht30_errors: 0,
            i2c,
            has_ina237: false,
        }));

        {
            let mut lock = state.lock().await;
            match lock.init_i2c_ina237().await {
                Ok(_) => {
                    info!("Found INA237 Power Meter");
                    lock.has_ina237 = true;
                }
                Err(e) => {
                    error!("INA237 Power Meter NOT FOUND: {:?}", e);
                }
            }
        }
        Ok(AppState { state })
    }
}
impl Deref for AppState {
    type Target = Mutex<State>;
    fn deref(&self) -> &Self::Target {
        self.state
    }
}

pub struct State {
    adc_temp_sensor: &'static mut adc_temp_sensor::Sensor<'static>,
    count: usize,
    pub sht30_errors: usize,
    pub i2c: I2c<'static, I2C0, Async>,
    pub has_ina237: bool,
}
struct I2CReading {
    temperature: f32,
    humidity: f32,
    heater_status: bool,
    humidity_tracking_alert: bool,
    temperature_tracking_alert: bool,
    command_status_success: bool,
    write_data_checksum_status: bool,
}

impl State {
    async fn read_i2c_sht30(&mut self) -> Result<I2CReading, embassy_rp::i2c::Error> {
        self.i2c.write_async(SHT30_ADDR, SHT30_CLEAR_STATUS).await?;
        self.i2c
            .write_async(SHT30_ADDR, SHT30_HIG_REP_CLOCK_STRETCH_READ)
            .await?;

        // Read 6 bytes of data
        let mut buffer = [0u8; 6];
        self.i2c.read_async(SHT30_ADDR, &mut buffer).await?;

        // Parse temperature data (first 3 bytes)
        let temp_raw = ((buffer[0] as u16) << 8) | (buffer[1] as u16);
        // Skip CRC check for simplicity (buffer[2] is CRC)

        // Parse humidity data (next 3 bytes)
        let hum_raw = ((buffer[3] as u16) << 8) | (buffer[4] as u16);
        // Skip CRC check for simplicity (buffer[5] is CRC)

        // Convert to actual values
        let temperature = -45.0 + 175.0 * (temp_raw as f32) / 65535.0;
        let humidity = 100.0 * (hum_raw as f32) / 65535.0;

        self.i2c.write_async(SHT30_ADDR, SHT30_READ_STATUS).await?;
        self.i2c.read_async(SHT30_ADDR, &mut buffer).await?;

        let status: u16 = ((buffer[0] as u16) << 8) | (buffer[1] as u16);

        let heater_status = status & 0b010000000000000 != 0;
        let humidity_tracking_alert = status & 0b000100000000000 != 0;
        let temperature_tracking_alert = status & 0b000010000000000 != 0;
        let command_status_success = status & 0b000000000000010 != 0;
        let write_data_checksum_status = status & 0b000000000000001 != 0;

        Ok(I2CReading {
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

#[embassy_executor::task(pool_size = 16)]
pub async fn web_task(id: usize, stack: &'static Stack<'static>, app_state: &'static AppState) {
    let app = picoserve::Router::new().route("/metrics", get(metrics));

    if let Err(e) = app_state.state.lock().await.read_i2c_sht30().await {
        error!("Got error reading i2c: {:?}", e);
    }

    loop {
        let config = picoserve::Config::new(picoserve::Timeouts {
            start_read_request: Some(Duration::from_secs(5)),
            persistent_start_read_request: Some(Duration::from_secs(1)),
            read_request: Some(Duration::from_secs(1)),
            write: Some(Duration::from_secs(1)),
        });

        let mut rx_buffer = [0; 2024];
        let mut tx_buffer = [0; 2024];
        let mut http_buffer = [0; 4048];
        let _ = picoserve::listen_and_serve_with_state(
            id,
            &app,
            &config,
            *stack,
            80,
            &mut rx_buffer,
            &mut tx_buffer,
            &mut http_buffer,
            &app_state,
        )
        .await;
    }
}
