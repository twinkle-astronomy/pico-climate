# pico-climate: Temperature & Humidity Sensor Prometheus Exporter

A Raspberry Pi Pico-based temperature and humidity sensor that exports readings as prometheus metrics.

Provides the following metrics
```
# HELP http_request_count Number of http requests recieved
# TYPE http_request_count counter
http_request_count{} 2
# HELP adc_temp_sensor Value of onboard temp sensor
# TYPE adc_temp_sensor gauge
adc_temp_sensor{unit="C"} 28.847847
adc_temp_sensor{unit="volts"} 0.7028198
adc_temp_sensor{unit="raw"} 875
# HELP sth30_reading Reading from STH30 Sensor
# TYPE sth30_reading gauge
sth30_reading{sensor="temperature"} 23.194855
sth30_reading{sensor="humidity"} 45.282673
# HELP sth30_status STH30 Status Registers
# TYPE sth30_status gauge
sth30_status{feature="heater_status"} 0
sth30_status{feature="humidity_tracking_alert"} 0
sth30_status{feature="temperature_tracking_alert"} 0
sth30_status{feature="command_status_success"} 0
sth30_status{feature="write_data_checksum_status"} 0
# HELP sth30_error Errors reading from STH30 Sensor
# TYPE sth30_error counter
sth30_error{} 0
```

## Prerequisites

- Docker and Docker Compose installed
- A Raspberry Pi Pico W board
- An STH30 Temperature/Humidity sensor wired to I2C bus 0
- USB cable to connect the Pico
- Debug probe [optional]

## Quick Start

1. **Clone or create your project directory:**

   ```bash
   mkdir pico-climate && cd pico-climate
   ```

2. Create a .env file with your WIFI_SSID and WIFI_PASSWORD

   ```
   WIFI_SSID=YOUR_SSID
   WIFI_PASSWORD=YOUR_WIFI_PASSWORD
   ```

3. **Build and start the development container:**
   
   ```bash
   docker compose run dev bash
   ```

5. **Run in debug mode using probe**
   
   ```bash
   cargo run
   ```
   The debug log will give you the hostname used to join the network.

7. **Connect to prometheus**
   
   The pico will boot up and join the configured wifi network.  Its dhcp lease will have a hostname like `pico-climate-ID`.  Find it in your router, and add a job to your prometheus config.  You can also hit the metrics endpoint with `curl -i http://NETWORK_LOCATION/metrics`
   Example prometheus config:
   ```
   scrape_configs:
     - job_name: 'pico-climate'
       static_configs:
         - targets:
           - 'pico-climate-e38a2a2a.lan:80'
           labels:
             location: "Office"
   ```

## Flashing Your Pico

### Method 1: Debug Probe

1. Connect a debug probe (like another Pico running picoprobe firmware)
2. Run:
   ```bash
   cargo run [--release]
   ```

### Method 2: USB Mass Storage (UF2, no probe required)

1. Hold the BOOTSEL button on your Pico and connect it via USB
2. The Pico should appear as a USB drive
3. Inside the container run:
   ```bash
   ./release_uf2.sh
   ```
4. Copy target/thumbv6m-none-eabi/release/pico-climate.uf2 to Pico drive.


### Container Management
```bash
# Start container in background
docker-compose up -d

# Enter running container
docker-compose exec dev bash

# Stop container
docker-compose down

# Rebuild container (after Dockerfile changes)
docker-compose up --build -d
```


## Resources

- [Embassy Documentation](https://embassy.dev/)
- [Raspberry Pi Pico Documentation](https://www.raspberrypi.org/documentation/microcontrollers/)
- [Rust Embedded Book](https://doc.rust-lang.org/stable/embedded-book/)
- [Embassy Examples](https://github.com/embassy-rs/embassy/tree/main/examples/rp)
