use defmt::info;
use embassy_net::Stack;
use embassy_time::Duration;
use picoserve::response::chunked::ChunkedResponse;
use picoserve::response::IntoResponse;
use picoserve::routing::get;

use defmt_rtt as _;
use static_cell::StaticCell;

use crate::prometheus::{self, MetricFamily, MetricsResponse};
use crate::{adc_temp_sensor, Mutex};

static METRICS: [MetricFamily; 2] = [
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
];

static COUNTER_SAMPLES: [prometheus::Sample; 1] = [prometheus::Sample::new(&[], 0.)];

static ADC_TEMP_SAMPLES: [prometheus::Sample; 3] = [
    prometheus::Sample::new(&["C"], 0.),
    prometheus::Sample::new(&["volts"], 0.),
    prometheus::Sample::new(&["raw"], 0.),
];

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

    ChunkedResponse::new(MetricsResponse::new(&METRICS))
}

#[derive(Clone, Copy)]
struct AppState {
    state: &'static Mutex<State>,
}

struct State {
    adc_temp_sensor: &'static mut adc_temp_sensor::Sensor<'static>,
    count: usize,
}

impl AppState {
    fn new(adc_temp_sensor: &'static mut adc_temp_sensor::Sensor<'static>) -> Self {
        static STATE: StaticCell<Mutex<State>> = StaticCell::new();
        let state = STATE.init(Mutex::new(State {
            count: 0,
            adc_temp_sensor,
        }));

        AppState { state }
    }
}

#[embassy_executor::task]
pub async fn web_task(
    stack: &'static Stack<'static>,
    adc_temp_sensor: &'static mut adc_temp_sensor::Sensor<'static>,
) {
    let app = picoserve::Router::new().route("/metrics", get(metrics));
    static STATE: StaticCell<AppState> = StaticCell::new();
    let app_state = STATE.init(AppState::new(adc_temp_sensor));

    loop {
        let config = picoserve::Config::new(picoserve::Timeouts {
            start_read_request: Some(Duration::from_secs(5)),
            persistent_start_read_request: Some(Duration::from_secs(1)),
            read_request: Some(Duration::from_secs(1)),
            write: Some(Duration::from_secs(1)),
        });

        let mut rx_buffer = [0; 1024];
        let mut tx_buffer = [0; 1024];
        let mut http_buffer = [0; 2048];
        let _ = picoserve::listen_and_serve_with_state(
            1,
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
