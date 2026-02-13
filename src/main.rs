#![no_std]
#![no_main]
use embassy_rp::multicore::spawn_core1;
use pico_climate::ina237::INA237_DEFAULT_ADDR;

use cyw43::{JoinOptions, ScanOptions};
use cyw43_pio::PioSpi;
use embassy_executor::{Executor, Spawner};
use embassy_rp::adc::{Adc, Channel};
use embassy_rp::i2c::{self, I2c};
use embassy_rp::multicore::Stack as MulticoreStack;
use embassy_rp::peripherals::{DMA_CH0, I2C0, I2C1, PIO0};
use embassy_rp::watchdog::Watchdog;
use embassy_rp::{
    bind_interrupts,
    gpio::{Level, Output},
    pio::{InterruptHandler, Pio},
};
use embassy_time::{Duration, Timer};
use panic_probe as _;
use pico_climate::http::{web_task, AppState, LAST_REQUEST_TIME};
use pico_climate::ina237::{continuous_reading, Ina237};
use pico_climate::sht30::Sht30Device;
use pico_climate::{adc_temp_sensor, sht30, Mutex, I2C_BUS_0};
// use pico_climate::tcp_logger::tcp_logger_task;
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use static_cell::StaticCell;

use core::fmt::Write;
use embassy_net::{Config as NetConfig, DhcpConfig, Stack};
use embassy_rp::clocks::RoscRng;

use defmt::{self as _, debug, info};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
    ADC_IRQ_FIFO => embassy_rp::adc::InterruptHandler;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    I2C1_IRQ => i2c::InterruptHandler<I2C1>;
});

static INA237: StaticCell<Ina237<I2cDevice<'static, CriticalSectionRawMutex, pico_climate::I2c0>>> =
    StaticCell::new();
static SHT30: StaticCell<
    sht30::Sht30Device<I2cDevice<'static, CriticalSectionRawMutex, pico_climate::I2c0>>,
> = StaticCell::new();
static SHT30_STATE: Mutex<sht30::SharedState> = Mutex::new(sht30::SharedState::new());
static INA237_STATE: Mutex<pico_climate::ina237::SharedState> =
    Mutex::new(pico_climate::ina237::SharedState::new());

defmt::timestamp!("{=u64:us}", embassy_time::Instant::now().as_micros());

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

fn create_unique_hostname(uid: [u8; 8]) -> heapless::String<32> {
    let mut hostname = heapless::String::new();
    write!(
        &mut hostname,
        "pico-climate-{:02x}{:02x}{:02x}{:02x}",
        uid[4], uid[5], uid[6], uid[7]
    )
    .unwrap();
    hostname
}

#[embassy_executor::task]
async fn watchdog_feeder(mut watchdog: Watchdog) {
    // Require a request in the last 2 minutes.
    loop {
        let elapsed = LAST_REQUEST_TIME.lock().await.elapsed();
        debug!("elapsed: {}", elapsed);
        if elapsed < Duration::from_secs(120) {
            debug!("Feeding the watchdog");
            watchdog.feed();
        }
        Timer::after(Duration::from_secs(1)).await;
    }
}
static mut CORE1_STACK: MulticoreStack<4096> = MulticoreStack::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    info!("Booting!");
    {
        let mut watchdog = Watchdog::new(p.WATCHDOG);
        watchdog.start(Duration::from_secs(5));
        spawner.spawn(watchdog_feeder(watchdog)).unwrap();
    }

    //Onboard temp sensor
    let adc = Adc::new(p.ADC, Irqs, embassy_rp::adc::Config::default());
    let temp_sensor = Channel::new_temp_sensor(p.ADC_TEMP_SENSOR);
    static TEMP_SENSOR: StaticCell<adc_temp_sensor::Sensor> = StaticCell::new();
    let temp_sensor = TEMP_SENSOR.init(adc_temp_sensor::Sensor { temp_sensor, adc });

    let i2c_bus0 = I2C_BUS_0.init(Mutex::new(I2c::new_async(
        p.I2C0,
        p.PIN_5,
        p.PIN_4,
        Irqs,
        i2c::Config::default(),
    )));
    let sht30_device = Sht30Device::new(I2cDevice::new(i2c_bus0), sht30::SHT30_ADDR);

    let ina237_device = Ina237::new(I2cDevice::new(i2c_bus0), INA237_DEFAULT_ADDR)
        .await
        .ok();

    let has_ina237 = ina237_device.is_some();

    spawn_core1(
        p.CORE1,
        unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) },
        move || {
            let executor1 = EXECUTOR1.init(Executor::new());
            executor1.run(|spawner| {
                spawner.must_spawn(sht30::continuous_reading(
                    SHT30.init(sht30_device),
                    &SHT30_STATE,
                ));
                if let Some(device) = ina237_device {
                    spawner.must_spawn(continuous_reading(INA237.init(device), &INA237_STATE));
                }
            });
        },
    );

    let mut flash =
        embassy_rp::flash::Flash::<_, embassy_rp::flash::Async, { 2 * 1024 * 1024 }>::new(
            p.FLASH, p.DMA_CH1,
        );
    let mut uid = [0u8; 8];
    flash.blocking_unique_id(&mut uid).unwrap();

    let fw = include_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../cyw43-firmware/43439A0_clm.bin");

    // Set up the WiFi chip communication via PIO
    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        cyw43_pio::RM2_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        p.DMA_CH0,
    );

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;
    let _ = spawner.spawn(cyw43_task(runner));

    control.init(clm).await;
    control.gpio_set(0, true).await;

    control
        .set_power_management(cyw43::PowerManagementMode::Performance)
        .await;

    info!("Set power management to Performance");

    let wifi_ssid = env!("WIFI_SSID");
    let wifi_password = env!("WIFI_PASSWORD");
    let seed: u64 = RoscRng.next_u64();

    let mut dhcp_config = DhcpConfig::default();
    dhcp_config.hostname = Some(create_unique_hostname(uid));
    let net_config = NetConfig::dhcpv4(dhcp_config);

    static RESOURCES: StaticCell<embassy_net::StackResources<16>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        net_device,
        net_config,
        RESOURCES.init(embassy_net::StackResources::new()),
        seed,
    );
    spawner.must_spawn(net_task(runner));
    static WEB_STACK: StaticCell<Stack<'_>> = StaticCell::new();
    let stack = WEB_STACK.init(stack);

    static APP_STATE: StaticCell<AppState> = StaticCell::new();

    let ina237_state = if has_ina237 {
        Some(&INA237_STATE)
    } else {
        None
    };

    let app_state = APP_STATE.init(
        AppState::new(temp_sensor, ina237_state, &SHT30_STATE)
            .await
            .unwrap(),
    );

    // spawner.must_spawn(tcp_logger_task(stack, "ryzen.lan", 9091));
    for id in 0..4 {
        spawner.must_spawn(web_task(id, stack, app_state));
    }

    loop {
        control.gpio_set(0, true).await;
        info!("Joining wifi {}", wifi_ssid);
        while let Err(_) = control
            .join(wifi_ssid, JoinOptions::new(wifi_password.as_bytes()))
            .await
        {
            for _ in 0..5 {
                control.gpio_set(0, false).await;
                Timer::after(Duration::from_millis(100)).await;

                control.gpio_set(0, true).await;
                Timer::after(Duration::from_millis(100)).await;
            }
        }

        stack.wait_link_up().await;
        info!("Link up");
        stack.wait_config_up().await;
        control.gpio_set(0, false).await;

        info!("Stack configured");
        info!("Hostname: '{}'", create_unique_hostname(uid));
        info!("Network Config: {}", stack.config_v4());

        embassy_futures::select::select(stack.wait_link_down(), async {
            loop {
                let mut scan_opts = ScanOptions::default();
                scan_opts.ssid = Some(heapless::String::try_from(wifi_ssid).unwrap());

                let mut scan = control.scan(scan_opts).await;
                loop {
                    let s = match scan.next().await {
                        Some(s) => s,
                        None => break,
                    };
                    let channel = s.chanspec & 0xff;

                    app_state.lock().await.wifi_signal[(channel as usize - 1) + 14 * 0]
                        .sample(-s.rssi as f32);
                    app_state.lock().await.wifi_signal[(channel as usize - 1) + 14 * 1]
                        .sample(-s.phy_noise as f32);
                    app_state.lock().await.wifi_signal[(channel as usize - 1) + 14 * 2]
                        .sample((s.rssi - s.phy_noise as i16) as f32);
                }
            }
        })
        .await;
    }
}
