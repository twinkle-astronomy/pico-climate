use defmt::{error, info};
use embassy_futures::block_on;
use embassy_net::{tcp::TcpSocket, Stack};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};
use embassy_time::{Duration, Timer};

#[defmt::global_logger]
struct Logger;

static SHARED_CHANNEL: Channel<CriticalSectionRawMutex, u8, 1024> = Channel::new();
static SHARED_LOCK: Mutex<CriticalSectionRawMutex, bool> = Mutex::new(false);
static RTT_ENCODER: Mutex<CriticalSectionRawMutex, defmt::Encoder> =
    Mutex::new(defmt::Encoder::new());

unsafe impl defmt::Logger for Logger {
    fn acquire() {
        loop {
            if let Ok(mut lock) = SHARED_LOCK.try_lock() {
                if *lock == false {
                    *lock = true;
                    break;
                }
            }
        }
        block_on(RTT_ENCODER.lock()).start_frame(|bytes| {
            for b in bytes {
                SHARED_CHANNEL.sender().try_send(*b).unwrap();
            }
        });
    }

    unsafe fn flush() {}

    unsafe fn release() {
        loop {
            if let Ok(mut lock) = SHARED_LOCK.try_lock() {
                if *lock == true {
                    *lock = false;
                    break;
                }
            }
        }

        block_on(RTT_ENCODER.lock()).end_frame(|bytes| {
            for byte in bytes {
                block_on(SHARED_CHANNEL.sender().send(*byte));
            }
        });
    }

    unsafe fn write(bytes: &[u8]) {
        block_on(RTT_ENCODER.lock()).write(bytes, |bytes| {
            for byte in bytes {
                block_on(SHARED_CHANNEL.sender().send(*byte));
            }
        });
    }
}

/// Task that connects to a TCP server and sends canned defmt messages
#[embassy_executor::task]
pub async fn tcp_logger_task(
    stack: &'static Stack<'static>,
    server_addr: &'static str,
    server_port: u16,
) -> ! {
    let mut rx_buffer = [0; 0];
    let mut tx_buffer = [0; 1024];
    info!("TCP Logger: Starting task");
    info!("TCP Logger: Target server {}:{}", server_addr, server_port);
    loop {
        stack.wait_config_up().await;
        info!("TCP Logger: Network is up, attempting connection");

        let addr = match stack
            .dns_query(server_addr, embassy_net::dns::DnsQueryType::A)
            .await
        {
            Ok(addresses) => addresses[0],
            Err(_) => {
                error!("TCP Logger: Failed to lookup address: {}", server_addr);
                Timer::after(Duration::from_secs(5)).await;
                continue;
            }
        };

        let mut socket = TcpSocket::new(*stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10)));
        socket.set_keep_alive(Some(Duration::from_secs(1)));

        let remote_endpoint = embassy_net::IpEndpoint::new(addr.into(), server_port);

        // Attempt to connect
        match socket.connect(remote_endpoint).await {
            Ok(()) => {
                info!("TCP Logger: Connected to {}:{}", server_addr, server_port);

                loop {
                    let receiver = SHARED_CHANNEL.receiver();
                    receiver.ready_to_receive().await;

                    let byte = receiver.try_peek().unwrap();

                    match socket.write(&[byte]).await {
                        Ok(_) => {
                            receiver.try_receive().unwrap();
                        }
                        Err(_) => break,
                    }
                }

                socket.close();
            }
            Err(e) => {
                error!("TCP Logger: Connection failed: {:?}", e);
            }
        }

        // Wait before reconnecting
        info!("TCP Logger: Waiting before reconnect");
        Timer::after(Duration::from_secs(5)).await;
    }
}
