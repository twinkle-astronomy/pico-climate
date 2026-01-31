use core::ops::Deref;

use defmt::{error, info};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{with_timeout, Duration, Instant};
use embedded_hal_async::i2c::I2c;
use picoserve::response::chunked::ChunkedResponse;
use picoserve::response::IntoResponse;
use picoserve::routing::get;

use static_cell::StaticCell;

use crate::ina237::{Ina237, INA237_DEFAULT_ADDR};
use crate::prometheus::sample::Sample;
use crate::prometheus::{
    counter, gauge, histogram, HistogramSamples, MetricWriter, MetricsRender, MetricsResponse,
};
use crate::sht30::{Sht30Device, SHT30_ADDR};
use crate::{adc_temp_sensor, Mutex};
use crate::{sht30, I2c0, I2c0Bus};

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

        match with_timeout(Duration::from_secs(1), app_state_lock.sht30.read()).await {
            Ok(Ok(sht30::Reading {
                temperature,
                humidity,
                heater_status,
                humidity_tracking_alert,
                temperature_tracking_alert,
                command_status_success,
                write_data_checksum_status,
            })) => {
                chunk_writer
                    .write(gauge(
                        "sht30_reading",
                        "Reading from SHT30 Sensor",
                        ["sensor"],
                        [
                            Sample::new(["temperature"], temperature),
                            Sample::new(["humidity"], humidity),
                        ]
                        .iter(),
                    ))
                    .await?;

                chunk_writer
                    .write(gauge(
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
                        ]
                        .iter(),
                    ))
                    .await?;
            }
            Ok(Err(e)) => {
                error!("Error reading from sht30: {:?}", e);
                app_state_lock.sht30_errors += 1;
            }
            Err(e) => {
                error!("Got error reading from sht30: {:?}", e);
                app_state_lock.sht30_errors += 1;
            }
        };

        chunk_writer
            .write(counter(
                "sht30_error",
                "Errors reading from SHT30 Sensor",
                [],
                [Sample::new([], app_state_lock.sht30_errors as f32)].iter(),
            ))
            .await?;

        if app_state_lock.has_ina237 {
            match with_timeout(Duration::from_secs(1), app_state_lock.ina237.read()).await {
                Ok(Ok(reading)) => {
                    chunk_writer
                        .write(gauge(
                            "ina237_reading",
                            "register values from INA237 Sensor",
                            ["register"],
                            [
                                Sample::new(["bus_voltage"], reading.bus_voltage),
                                Sample::new(["shunt_voltage"], reading.shunt_voltage),
                                Sample::new(["current"], reading.current),
                                Sample::new(["power"], reading.power),
                                Sample::new(["die_temperature"], reading.die_temperature),
                            ]
                            .iter(),
                        ))
                        .await?
                }
                Ok(Err(e)) => {
                    error!("Error reading from ina237: {:?}", e);
                    app_state_lock.ina237_errors += 1
                }

                Err(e) => {
                    error!("Error reading from ina237: {:?}", e);
                    app_state_lock.ina237_errors += 1
                }
            };

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

static STATE: StaticCell<Mutex<State<I2cDevice<'static, CriticalSectionRawMutex, I2c0>>>> =
    StaticCell::new();

#[derive(Clone, Copy)]
pub struct AppState {
    state: &'static Mutex<State<I2cDevice<'static, CriticalSectionRawMutex, I2c0>>>,
}

impl AppState {
    pub async fn new(
        adc_temp_sensor: &'static mut adc_temp_sensor::Sensor<'static>,
        i2c_bus: &'static I2c0Bus,
    ) -> Result<Self, embassy_rp::i2c::Error> {
        let state = STATE.init(Mutex::new(State {
            count: [Sample::new([], 0.)],
            adc_temp_sensor,
            sht30_errors: 0,
            ina237_errors: 0,
            i2c: I2cDevice::new(&i2c_bus),
            sht30: Sht30Device::new(I2cDevice::new(&i2c_bus), SHT30_ADDR),
            ina237: Ina237::new(I2cDevice::new(&i2c_bus), INA237_DEFAULT_ADDR),
            has_ina237: false,
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

        {
            let mut lock = state.lock().await;

            // Initialize SHT30 sensor
            if let Err(e) = lock.sht30.soft_reset().await {
                error!("Failed to initialize SHT30: {:?}", e);
            } else {
                info!("SHT30 initialized");
            }

            // Initialize INA237 power meter if present
            match with_timeout(Duration::from_secs(1), lock.ina237.init()).await {
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
    type Target = Mutex<State<I2cDevice<'static, CriticalSectionRawMutex, I2c0>>>;
    fn deref(&self) -> &Self::Target {
        self.state
    }
}

pub struct State<I: I2c> {
    adc_temp_sensor: &'static mut adc_temp_sensor::Sensor<'static>,
    count: [Sample<'static, 0>; 1],
    pub sht30_errors: usize,
    pub ina237_errors: usize,
    pub i2c: I,
    pub sht30: Sht30Device<I>,
    pub ina237: Ina237<I>,
    pub has_ina237: bool,
    pub wifi_signal: [HistogramSamples<'static, 3, 11>; 14 * 3],
}

#[embassy_executor::task(pool_size = 8)]
pub async fn web_task(id: usize, stack: &'static Stack<'static>, app_state: &'static AppState) {
    let app = picoserve::Router::new()
        .route("/metrics", get(metrics))
        .with_state(app_state);

    if let Err(e) = app_state.state.lock().await.sht30.read().await {
        error!("Got error reading i2c: {:?}", e);
    }

    loop {
        let config = picoserve::Config::new(picoserve::Timeouts {
            start_read_request: Some(Duration::from_secs(5)),
            persistent_start_read_request: Some(Duration::from_secs(1)),
            read_request: Some(Duration::from_secs(1)),
            write: Some(Duration::from_secs(1)),
        });

        let mut rx_buffer = [0; 1012];
        let mut tx_buffer = [0; 1012];
        let mut http_buffer = [0; 1012];
        let _ = picoserve::Server::new(&app, &config, &mut http_buffer)
            .listen_and_serve(id, *stack, 80, &mut rx_buffer, &mut tx_buffer)
            .await;
    }
}
