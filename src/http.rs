use embassy_net::Stack;
use embassy_time::Duration;
use panic_probe as _;
use picoserve::routing::get;

use defmt_rtt as _;

fn build_app() -> picoserve::Router<impl picoserve::routing::PathRouter> {
    picoserve::Router::new().route("/", get(|| async move { "Hello World\n" }))
}

#[embassy_executor::task]
pub async fn web_task(stack: &'static Stack<'static>) {
    let app = build_app();

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
        let _ = picoserve::listen_and_serve(
            1,
            &app,
            &config,
            *stack,
            80,
            &mut rx_buffer,
            &mut tx_buffer,
            &mut http_buffer,
        )
        .await;
    }
}
