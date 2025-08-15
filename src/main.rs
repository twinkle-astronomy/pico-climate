#![no_std]
#![no_main]

use cyw43::JoinOptions;
use cyw43_pio::PioSpi;
// use defmt::*;
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_rp::peripherals::{DMA_CH0, PIO0};
use embassy_rp::{
    bind_interrupts,
    gpio::{Level, Output},
    pio::{InterruptHandler, Pio},
};
use embassy_time::{Duration, Timer};
use panic_probe as _;
use static_cell::StaticCell;

use embassy_rp::clocks::RoscRng;
use core::str::FromStr;
use embassy_net::{Config as NetConfig, DhcpConfig, Stack};
use picoserve::{
    routing::get,
};

use defmt::{self as _, info};
use defmt_rtt as _;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

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


fn build_app() -> picoserve::Router<impl picoserve::routing::PathRouter> {
    picoserve::Router::new().route("/", get(|| async move { "Hello World" }))
}

#[embassy_executor::task]
async fn web_task(stack: &'static Stack<'static>) {

    loop {
        let mut rx_buffer = [0; 1024];
        let mut tx_buffer = [0; 1024];
        let mut socket = TcpSocket::new(*stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10)));

        if let Err(_) = socket.accept(80).await {
            continue;
        }
        // Method 1: Simple router with global state
        let app = build_app();
        
        let config = 
            picoserve::Config::new(picoserve::Timeouts {
                start_read_request: Some(Duration::from_secs(5)),
                persistent_start_read_request: Some(Duration::from_secs(1)),
                read_request: Some(Duration::from_secs(1)),
                write: Some(Duration::from_secs(1)),
            });

        let mut http_buffer = [0; 2048];
        let _ = picoserve::serve(&app, &config, &mut http_buffer, socket).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    info!("Booting!");

    let fw = include_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../cyw43-firmware/43439A0_clm.bin");

    // Set up the WiFi chip communication via PIO
    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        cyw43_pio::DEFAULT_CLOCK_DIVIDER,
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
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    info!("Set power management to PowerSave");

    control.gpio_set(0, false).await;

    let wifi_ssid = env!("WIFI_SSID");
    let wifi_password = env!("WIFI_PASSWORD");
    const CLIENT_NAME: &str = "pico-climate";
    let seed: u64 = RoscRng.next_u64();

    let mut dhcp_config = DhcpConfig::default();
    dhcp_config.hostname = Some(heapless::String::from_str(CLIENT_NAME).unwrap());
    let net_config = NetConfig::dhcpv4(dhcp_config);

    static RESOURCES: StaticCell<embassy_net::StackResources<32>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        net_device,
        net_config,
        RESOURCES.init(embassy_net::StackResources::new()),
        seed
        );
    let _ = spawner.spawn(net_task(runner));
    info!("Joining wifi {}", wifi_ssid);
    while let Err(_) = control
        .join(wifi_ssid, JoinOptions::new(wifi_password.as_bytes()))
        .await
    {
        for _ in 0..10 {
            control.gpio_set(0, true).await;
            Timer::after(Duration::from_millis(100)).await;

            control.gpio_set(0, false).await;
            Timer::after(Duration::from_millis(100)).await;
        }
    }

    stack.wait_link_up().await;
    info!("Link up");
    stack.wait_config_up().await;
    info!("Stack configured");

    static WEB_STACK: StaticCell<Stack<'_>> = StaticCell::new();
    let stack = WEB_STACK.init(stack);
    let _ = spawner.spawn(web_task(stack));

}
