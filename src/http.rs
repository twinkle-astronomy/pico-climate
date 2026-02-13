use core::ops::Deref;

use defmt::info;
use embassy_net::Stack;
use embassy_time::{Duration, Instant};
use picoserve::response::chunked::ChunkedResponse;
use picoserve::response::IntoResponse;
use picoserve::routing::get;

use static_cell::StaticCell;

use crate::ina237;
use crate::prometheus::sample::Sample;
use crate::prometheus::{
    counter, gauge, histogram, HistogramSamples, MetricWriter, MetricsRender, MetricsResponse,
};
use crate::sht30;
use crate::{adc_temp_sensor, Mutex};

pub static LAST_REQUEST_TIME: Mutex<Instant> = Mutex::new(Instant::MIN);

struct PicoClimateMetrics {
    app_state: AppState,
}

impl MetricsRender for PicoClimateMetrics {
    async fn write_chunks<W>(
        &self,
        chunk_writer: &mut picoserve::response::chunked::ChunkWriter<W>,
    ) -> Result<(), W::Error>
    where
        W: picoserve::io::Write,
    {
        let mut app_state_lock = self.app_state.state.lock().await;
        app_state_lock.count[0].incr(1.);

        chunk_writer
            .write(counter(
                "http_request_count",
                "Number of http requests recieved",
                [],
                app_state_lock.count.iter(),
            ))
            .await?;

        chunk_writer
            .write(histogram(
                "wifi_signal_strength",
                "Wifi signal strength",
                ["ssid", "channel", "metric"],
                app_state_lock.wifi_signal.iter(),
            ))
            .await?;

        if let Ok(adc_sample) = app_state_lock.adc_temp_sensor.read().await {
            chunk_writer
                .write(gauge(
                    "adc_temp_sensor",
                    "Value of onboard temp sensor",
                    ["unit"],
                    [
                        Sample::new(["C"], adc_sample.temp_celsius),
                        Sample::new(["volts"], adc_sample.volt),
                        Sample::new(["raw"], adc_sample.raw as f32),
                    ]
                    .iter(),
                ))
                .await?;
        }

        let sht30_output = app_state_lock.sht30_state.lock().await.snapshot();

        chunk_writer
            .write(gauge(
                "sht30_reading",
                "Reading from SHT30 Sensor",
                ["sensor"],
                [
                    Sample::new(["temperature"], sht30_output.temperature),
                    Sample::new(["humidity"], sht30_output.humidity),
                ]
                .iter(),
            ))
            .await?;

        chunk_writer
            .write(counter(
                "sht30_status_count",
                "Number of times SHT30 Status Registers have been true",
                ["feature"],
                [
                    Sample::new(["heater_status"], sht30_output.heater_status_count),
                    Sample::new(
                        ["humidity_tracking_alert"],
                        sht30_output.humidity_tracking_alert_count,
                    ),
                    Sample::new(
                        ["temperature_tracking_alert"],
                        sht30_output.temperature_tracking_alert_count,
                    ),
                    Sample::new(
                        ["command_status_success"],
                        sht30_output.command_status_success_count,
                    ),
                    Sample::new(
                        ["write_data_checksum_status"],
                        sht30_output.write_data_checksum_status_count,
                    ),
                ]
                .iter(),
            ))
            .await?;

        chunk_writer
            .write(counter(
                "sht30_zeros",
                "Zero readings from SHT30 Sensor",
                [],
                [Sample::new([], sht30_output.zeros)].iter(),
            ))
            .await?;

        chunk_writer
            .write(counter(
                "sht30_successes",
                "Successful reads from SHT30 Sensor",
                [],
                [Sample::new([], sht30_output.successes)].iter(),
            ))
            .await?;

        chunk_writer
            .write(counter(
                "sht30_timeouts",
                "Timeout events reading SHT30 Sensor",
                [],
                [Sample::new([], sht30_output.timeouts)].iter(),
            ))
            .await?;

        chunk_writer
            .write(counter(
                "sht30_recoverable_errors",
                "Recoverable erors from SHT30 Sensor",
                [],
                [Sample::new([], sht30_output.recoverable_errors)].iter(),
            ))
            .await?;

        chunk_writer
            .write(counter(
                "sht30_error",
                "Errors reading from SHT30 Sensor",
                [],
                [Sample::new([], app_state_lock.sht30_errors as f32)].iter(),
            ))
            .await?;

        if let Some(ina237_state) = app_state_lock.ina237_state {
            let ina237_output = ina237_state.lock().await.snapshot();

            chunk_writer
                .write(gauge(
                    "ina237_reading",
                    "register values from INA237 Sensor",
                    ["register"],
                    [
                        Sample::new(["bus_voltage"], ina237_output.bus_voltage),
                        Sample::new(["shunt_voltage"], ina237_output.shunt_voltage),
                        Sample::new(["current"], ina237_output.current),
                        Sample::new(["power"], 0.),
                        Sample::new(["die_temperature"], 0.),
                    ]
                    .iter(),
                ))
                .await?;

            chunk_writer
                .write(counter(
                    "ina237_successes",
                    "Successful reads from ina237",
                    [],
                    [Sample::new([], ina237_output.successes)].iter(),
                ))
                .await?;

            chunk_writer
                .write(counter(
                    "ina237_timeouts",
                    "Timeout events reading ina237",
                    [],
                    [Sample::new([], ina237_output.timeouts)].iter(),
                ))
                .await?;

            chunk_writer
                .write(counter(
                    "ina237_zeros",
                    "Zeroes reading from ina237",
                    [],
                    [Sample::new([], ina237_output.zeros)].iter(),
                ))
                .await?;

            chunk_writer
                .write(counter(
                    "ina237_recoverable_errors",
                    "Recoverable errors from ina237",
                    [],
                    [Sample::new([], ina237_output.recoverable_errors)].iter(),
                ))
                .await?;

            chunk_writer
                .write(counter(
                    "ina237_errors",
                    "Errors reading from ina237",
                    [],
                    [Sample::new([], app_state_lock.ina237_errors as f32)].iter(),
                ))
                .await?;
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

    ChunkedResponse::new(MetricsResponse::new(PicoClimateMetrics { app_state }))
}

static STATE: StaticCell<Mutex<State>> = StaticCell::new();

#[derive(Clone, Copy)]
pub struct AppState {
    state: &'static Mutex<State>,
}

impl AppState {
    pub async fn new(
        adc_temp_sensor: &'static mut adc_temp_sensor::Sensor<'static>,
        ina237_state: Option<&'static Mutex<ina237::SharedState>>,
        sht30_state: &'static Mutex<sht30::SharedState>,
    ) -> Result<Self, embassy_rp::i2c::Error> {
        let state = STATE.init(Mutex::new(State {
            count: [Sample::new([], 0.)],
            adc_temp_sensor,
            sht30_errors: 0,
            ina237_errors: 0,
            // i2c: I2cDevice::new(&i2c_bus),
            ina237_state,
            sht30_state,
            wifi_signal: [
                // RSSI
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "1", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "2", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "3", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "4", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "5", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "6", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "7", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "8", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "9", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "10", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "11", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "12", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "13", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "14", "rssi"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                // PHY_NOISE
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "1", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "2", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "3", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "4", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "5", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "6", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "7", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "8", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "9", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "10", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "11", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "12", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "13", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "14", "phy_noise"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                // SNR
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "1", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "2", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "3", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "4", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "5", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "6", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "7", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "8", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "9", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "10", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "11", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "12", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "13", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
                HistogramSamples::new(
                    [env!("WIFI_SSID"), "14", "snr"],
                    [
                        10.,
                        20.,
                        30.,
                        40.,
                        50.,
                        60.,
                        70.,
                        80.,
                        90.,
                        100.,
                        f32::INFINITY,
                    ],
                ),
            ],
        }));

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
    count: [Sample<'static, 0>; 1],
    pub sht30_errors: usize,
    pub ina237_errors: usize,
    // pub i2c: I,
    // pub sht30: Sht30Device<I>,
    pub ina237_state: Option<&'static Mutex<ina237::SharedState>>,
    pub sht30_state: &'static Mutex<sht30::SharedState>,
    pub wifi_signal: [HistogramSamples<'static, 3, 11>; 14 * 3],
}

#[embassy_executor::task(pool_size = 4)]
pub async fn web_task(id: usize, stack: &'static Stack<'static>, app_state: &'static AppState) {
    let app = picoserve::Router::new()
        .route("/metrics", get(metrics))
        .with_state(app_state);

    loop {
        let config = picoserve::Config::new(picoserve::Timeouts {
            start_read_request: Some(Duration::from_secs(5)),
            persistent_start_read_request: Some(Duration::from_secs(1)),
            read_request: Some(Duration::from_secs(1)),
            write: Some(Duration::from_secs(10)),
        });

        let mut rx_buffer = [0; 1024];
        let mut tx_buffer = [0; 4096];
        let mut http_buffer = [0; 1024];
        let _ = picoserve::Server::new(&app, &config, &mut http_buffer)
            .listen_and_serve(id, *stack, 80, &mut rx_buffer, &mut tx_buffer)
            .await;
    }
}
