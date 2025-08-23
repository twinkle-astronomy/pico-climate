use defmt::{error, info};
use embassy_net::Stack;
use embassy_rp::i2c::{Async, I2c};
use embassy_rp::peripherals::I2C0;
use embassy_time::Duration;
use picoserve::response::chunked::ChunkedResponse;
use picoserve::response::IntoResponse;
use picoserve::routing::get;

use defmt_rtt as _;
use static_cell::StaticCell;

use crate::prometheus::{self, MetricFamily, MetricsResponse};
use crate::{adc_temp_sensor, Mutex};

const SHT30_ADDR: u16 = 0x44;
const SHT30_HIG_REP_CLOCK_STRETCH_READ: [u8; 2] = [0x2C, 0x06];
const SHT30_READ_STATUS: [u8; 2] = [0xF3, 0x2D];
const SHT30_CLEAR_STATUS: [u8; 2] = [0x30, 0x41];

static METRICS: [MetricFamily; 5] = [
    MetricFamily::new(
        "http_request_count",
        "Number of http requests recieved",
        crate::prometheus::MetricType::Counter,
        &[],
        &COUNTER_SAMPLES,
    ),
    MetricFamily::new(
        "adc_temp_sensor",
        "Value of onboard temp sensor",
        crate::prometheus::MetricType::Gauge,
        &["unit"],
        &ADC_TEMP_SAMPLES,
    ),
    MetricFamily::new(
        "sth30_reading",
        "Reading from STH30 Sensor",
        crate::prometheus::MetricType::Gauge,
        &["sensor"],
        &STH30_SAMPLES,
    ),
    MetricFamily::new(
        "sth30_status",
        "STH30 Status Registers",
        crate::prometheus::MetricType::Gauge,
        &["feature"],
        &STH30_STATUSES,
    ),
    MetricFamily::new(
        "sth30_error",
        "Errors reading from STH30 Sensor",
        crate::prometheus::MetricType::Counter,
        &[],
        &STH30_ERRORS,
    ),
];

static COUNTER_SAMPLES: [prometheus::Sample; 1] = [prometheus::Sample::new(&[], 0.)];

static ADC_TEMP_SAMPLES: [prometheus::Sample; 3] = [
    prometheus::Sample::new(&["C"], 0.),
    prometheus::Sample::new(&["volts"], 0.),
    prometheus::Sample::new(&["raw"], 0.),
];

static STH30_SAMPLES: [prometheus::Sample; 2] = [
    prometheus::Sample::new(&["temperature"], 0.),
    prometheus::Sample::new(&["humidity"], 0.),
];

static STH30_STATUSES: [prometheus::Sample; 5] = [
    prometheus::Sample::new(&["heater_status"], 0.),
    prometheus::Sample::new(&["humidity_tracking_alert"], 0.),
    prometheus::Sample::new(&["temperature_tracking_alert"], 0.),
    prometheus::Sample::new(&["command_status_success"], 0.),
    prometheus::Sample::new(&["write_data_checksum_status"], 0.),
];

static STH30_ERRORS: [prometheus::Sample; 1] = [prometheus::Sample::new(&[], 0.)];

async fn metrics(
    picoserve::extract::State(app_state): picoserve::extract::State<AppState>,
) -> impl IntoResponse {
    info!("GET /metrics");

    // Update request count and metric
    let mut app_state_lock = app_state.state.lock().await;
    app_state_lock.count = app_state_lock.count + 1;
    COUNTER_SAMPLES[0].set(app_state_lock.count as f32);

    // Update ADC tempurature sensor samples.
    let adc_sample = app_state_lock.adc_temp_sensor.read().await.unwrap();

    ADC_TEMP_SAMPLES[0].set(adc_sample.temp_celsius);
    ADC_TEMP_SAMPLES[1].set(adc_sample.volt);
    ADC_TEMP_SAMPLES[2].set(adc_sample.raw as f32);

    match app_state_lock.read_i2c().await {
        Ok(I2CReading {
            temperature,
            humidity,
            heater_status,
            humidity_tracking_alert,
            temperature_tracking_alert,
            command_status_success,
            write_data_checksum_status,
        }) => {
            STH30_SAMPLES[0].set(temperature);
            STH30_SAMPLES[1].set(humidity);

            STH30_STATUSES[0].set(if heater_status { 1. } else { 0. });
            STH30_STATUSES[1].set(if humidity_tracking_alert { 1. } else { 0. });
            STH30_STATUSES[2].set(if temperature_tracking_alert { 1. } else { 0. });
            STH30_STATUSES[3].set(if command_status_success { 1. } else { 0. });
            STH30_STATUSES[4].set(if write_data_checksum_status { 1. } else { 0. });
        }
        Err(e) => {
            error!("Got error reading i2c: {:?}", e);
            STH30_ERRORS[0].incr(1.);
        }
    }

    ChunkedResponse::new(MetricsResponse::new(&METRICS))
}

#[derive(Clone, Copy)]
pub struct AppState {
    state: &'static Mutex<State>,
}

struct State {
    adc_temp_sensor: &'static mut adc_temp_sensor::Sensor<'static>,
    count: usize,
    i2c: I2c<'static, I2C0, Async>,
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
    async fn read_i2c(&mut self) -> Result<I2CReading, embassy_rp::i2c::Error> {
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

        let heater_status = status              & 0b010000000000000 != 0;
        let humidity_tracking_alert = status    & 0b000100000000000 != 0;
        let temperature_tracking_alert = status & 0b000010000000000 != 0;
        let command_status_success = status     & 0b000000000000010 != 0;
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
            i2c,
        }));

        Ok(AppState { state })
    }
}

#[embassy_executor::task(pool_size = 16)]
pub async fn web_task(
    id: usize,
    stack: &'static Stack<'static>,
    app_state: &'static AppState,
) {
    let app = picoserve::Router::new().route("/metrics", get(metrics));

    if let Err(e) = app_state.state.lock().await.read_i2c().await {
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
