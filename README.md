# pico-climate: Temperature & Humidity Sensor

A Raspberry Pi Pico-based temperature and humidity sensor built with Rust and Embassy framework, running in a Docker development environment.

## Prerequisites

- Docker and Docker Compose installed
- A Raspberry Pi Pico W board
- USB cable to connect the Pico
- Debug probe

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
   docker-compose up -d
   ```

4. **Enter the container:**
   ```bash
   docker-compose exec dev bash
   ```
5. ** Run in debug mode using probe**
   ```bash
   cargo run
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
