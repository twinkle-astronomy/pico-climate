use core::ops::Deref;

use defmt::{debug, error, info, warn};
use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_rp::i2c::{Async, I2c};
use embassy_rp::peripherals::I2C0;
use embassy_rp::watchdog::Watchdog;
use embassy_time::{Duration, Instant, Timer};
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

static COUNTER_SAMPLES: [prometheus::Sample; 1] = [prometheus::Sample::new(&[], 0.)];

static ADC_TEMP_SAMPLES: [prometheus::Sample; 3] = [
    prometheus::Sample::new(&["C"], 0.),
    prometheus::Sample::new(&["volts"], 0.),
    prometheus::Sample::new(&["raw"], 0.),
];

static SHT30_SAMPLES: [prometheus::Sample; 2] = [
    prometheus::Sample::new(&["temperature"], 0.),
    prometheus::Sample::new(&["humidity"], 0.),
];

static SHT30_STATUSES: [prometheus::Sample; 5] = [
    prometheus::Sample::new(&["heater_status"], 0.),
    prometheus::Sample::new(&["humidity_tracking_alert"], 0.),
    prometheus::Sample::new(&["temperature_tracking_alert"], 0.),
    prometheus::Sample::new(&["command_status_success"], 0.),
    prometheus::Sample::new(&["write_data_checksum_status"], 0.),
];

static INA237_SAMPLES: [prometheus::Sample; 5] = [
    prometheus::Sample::new(&["bus_voltage"], 0.),
    prometheus::Sample::new(&["shunt_voltage"], 0.),
    prometheus::Sample::new(&["current"], 0.),
    prometheus::Sample::new(&["power"], 0.),
    prometheus::Sample::new(&["die_temperature"], 0.),
];

static SHT30_ERRORS: [prometheus::Sample; 1] = [prometheus::Sample::new(&[], 0.)];

struct OptionalChain<T, S> {
    base_metrics: T,
    optional_metrics: Option<S>,
}

impl<T, S> Iterator for OptionalChain<T, S>
where
    T: Iterator<Item = MetricFamily>,
    S: Iterator<Item = MetricFamily>,
{
    type Item = MetricFamily;

    fn next(&mut self) -> Option<Self::Item> {
        match self.base_metrics.next() {
            Some(n) => Some(n),
            None => match &mut self.optional_metrics {
                Some(m) => m.next(),
                None => None,
            },
        }
    }
}

async fn metrics(
    picoserve::extract::State(app_state): picoserve::extract::State<AppState>,
) -> impl IntoResponse {
    info!("GET /metrics");
    {
        let mut last_req = LAST_REQUEST_TIME.lock().await;
        *last_req = Some(Instant::now());
    }

    // Update request count and metric
    let mut app_state_lock = app_state.state.lock().await;
    app_state_lock.count = app_state_lock.count + 1;
    COUNTER_SAMPLES[0].set(app_state_lock.count as f32);

    // Update ADC tempurature sensor samples.
    let adc_sample = app_state_lock.adc_temp_sensor.read().await.unwrap();

    ADC_TEMP_SAMPLES[0].set(adc_sample.temp_celsius);
    ADC_TEMP_SAMPLES[1].set(adc_sample.volt);
    ADC_TEMP_SAMPLES[2].set(adc_sample.raw as f32);

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
            SHT30_SAMPLES[0].set(temperature);
            SHT30_SAMPLES[1].set(humidity);

            SHT30_STATUSES[0].set(if heater_status { 1. } else { 0. });
            SHT30_STATUSES[1].set(if humidity_tracking_alert { 1. } else { 0. });
            SHT30_STATUSES[2].set(if temperature_tracking_alert { 1. } else { 0. });
            SHT30_STATUSES[3].set(if command_status_success { 1. } else { 0. });
            SHT30_STATUSES[4].set(if write_data_checksum_status { 1. } else { 0. });
        }
        Err(e) => {
            error!("Got error reading i2c: {:?}", e);
            SHT30_ERRORS[0].incr(1.);
        }
    }

    let metrics = [
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
            "sht30_reading",
            "Reading from SHT30 Sensor",
            crate::prometheus::MetricType::Gauge,
            &["sensor"],
            &SHT30_SAMPLES,
        ),
        MetricFamily::new(
            "sht30_status",
            "SHT30 Status Registers",
            crate::prometheus::MetricType::Gauge,
            &["feature"],
            &SHT30_STATUSES,
        ),
        MetricFamily::new(
            "sht30_error",
            "Errors reading from SHT30 Sensor",
            crate::prometheus::MetricType::Counter,
            &[],
            &SHT30_ERRORS,
        ),
    ]
    .into_iter();

    let optional = if app_state_lock.has_ina237 {
        if let Ok(reading) = app_state_lock.read_i2c_ina237().await {
            INA237_SAMPLES[0].set(reading.bus_voltage);
            INA237_SAMPLES[1].set(reading.shunt_voltage);
            INA237_SAMPLES[2].set(reading.current);
            INA237_SAMPLES[3].set(reading.power);
            INA237_SAMPLES[4].set(reading.die_temperature);
        }
        Some(
            [MetricFamily::new(
                "ina237_reading",
                "Reading from INA237 Sensor",
                crate::prometheus::MetricType::Gauge,
                &["sensor"],
                &INA237_SAMPLES,
            )]
            .into_iter(),
        )
    } else {
        None
    };
    ChunkedResponse::new(MetricsResponse::new(OptionalChain {
        base_metrics: metrics,
        optional_metrics: optional,
    }))
}

#[derive(Clone, Copy)]
pub struct AppState {
    state: &'static Mutex<State>,
}

static LAST_REQUEST_TIME: Mutex<Option<Instant>> = Mutex::new(None);

#[embassy_executor::task]
async fn watchdog_feeder(mut watchdog: Watchdog) {
    // Start hardware watchdog with max timeout (~8 seconds)
    watchdog.start(Duration::from_secs(5));

    loop {
        Timer::after(Duration::from_secs(1)).await; // Feed every 1 seconds

        // Check if we've had a recent HTTP request
        let should_reset = {
            let last_req = LAST_REQUEST_TIME.lock().await;
            match *last_req {
                Some(time) => time.elapsed() > Duration::from_secs(120), // 2 minutes
                None => false, // No requests yet, don't reset immediately
            }
        };

        if should_reset {
            // Don't feed the watchdog, let it reset the system
            warn!("No HTTP requests for 2 minutes, letting watchdog reset system");
            loop {
                Timer::after(Duration::from_secs(10)).await;
                // Just wait for hardware watchdog to trigger reset
            }
        } else {
            debug!("Feeding the watchdog");
            // Feed the watchdog to keep system alive
            watchdog.feed();
        }
    }
}

impl AppState {
    pub async fn new(
        adc_temp_sensor: &'static mut adc_temp_sensor::Sensor<'static>,
        mut i2c: I2c<'static, I2C0, Async>,
        watchdog: Watchdog,
        spawner: Spawner,
    ) -> Result<Self, embassy_rp::i2c::Error> {
        i2c.write_async(SHT30_ADDR, [0x30, 0xA2]).await?;

        // Set initial request time to now to prevent immediate reset
        {
            let mut last_req = LAST_REQUEST_TIME.lock().await;
            *last_req = Some(Instant::now());
        }

        spawner.spawn(watchdog_feeder(watchdog)).unwrap();

        static STATE: StaticCell<Mutex<State>> = StaticCell::new();
        let state = STATE.init(Mutex::new(State {
            count: 0,
            adc_temp_sensor,
            i2c,
            has_ina237: false,
            ina_state: InaState::new(),
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
pub struct InaState {
    pub device_start_time: Instant,
    pub last_reading_time: Option<Instant>,
}

impl InaState {
    pub fn new() -> Self {
        InaState {
            device_start_time: Instant::now(),
            last_reading_time: None,
        }
    }
}

pub struct State {
    adc_temp_sensor: &'static mut adc_temp_sensor::Sensor<'static>,
    count: usize,
    pub i2c: I2c<'static, I2C0, Async>,
    pub has_ina237: bool,
    pub ina_state: InaState,
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
