# pico-climate: Temperature & Humidity Sensor

A Raspberry Pi Pico-based temperature and humidity sensor built with Rust and Embassy framework, running in a Docker development environment.

## Prerequisites

- Docker and Docker Compose installed
- A Raspberry Pi Pico board
- USB cable to connect the Pico

## Quick Start

1. **Clone or create your project directory:**
   ```bash
   mkdir pico-climate && cd pico-climate
   ```

2. **Create all the configuration files** (use the artifacts provided above)

3. **Build and start the development container:**
   ```bash
   docker-compose up -d
   ```

4. **Enter the container:**
   ```bash
   docker-compose exec rust-embassy bash
   ```

5. **Run the setup script:**
   ```bash
   chmod +x setup.sh
   ./setup.sh
   ```

## Flashing Your Pico

### Method 1: USB Mass Storage (UF2)

1. Hold the BOOTSEL button on your Pico and connect it via USB
2. The Pico should appear as a USB drive
3. Inside the container, run:
   ```bash
   cargo run --release
   ```
4. The firmware will be automatically copied to the Pico

### Method 2: Debug Probe (Advanced)

1. Connect a debug probe (like another Pico running picoprobe firmware)
2. Modify `.cargo/config.toml` to use the probe-rs runner
3. Run:
   ```bash
   cargo run --release
   ```

## Project Structure

```
pico-climate/
├── Dockerfile              # Container definition
├── docker-compose.yml      # Container orchestration
├── Cargo.toml              # Project dependencies
├── build.rs                # Build script
├── memory.x                # Memory layout
├── setup.sh                # Setup script
├── .cargo/
│   └── config.toml         # Cargo configuration
└── src/
    └── main.rs             # Temperature/humidity sensor application
```

## What the Hello World Does

The example application:
- Prints "Hello World" messages via defmt (visible with debug probe)
- Blinks the onboard LED (GPIO 25) every second
- Uses Embassy's async runtime for efficient task handling
- Demonstrates basic GPIO control and timing

## Development Tips

### Building
```bash
# Debug build
cargo build

# Release build (smaller, optimized)
cargo build --release
```

### Debugging
If you have a debug probe connected:
```bash
# View debug output
probe-rs attach --chip RP2040

# Flash and immediately start debugging
probe-rs run --chip RP2040 target/thumbv6m-none-eabi/release/pico-embassy-hello
```

### Container Management
```bash
# Start container in background
docker-compose up -d

# Enter running container
docker-compose exec rust-embassy bash

# Stop container
docker-compose down

# Rebuild container (after Dockerfile changes)
docker-compose up --build -d
```

## Troubleshooting

### Permission Issues with USB
If you can't access the Pico via USB:
1. Make sure your user is in the `dialout` group on the host
2. Try running Docker with `--privileged` flag (already in docker-compose.yml)

### Build Errors
- Ensure all files are in the correct locations
- Try `cargo clean` and rebuild
- Check that `thumbv6m-none-eabi` target is installed

### Pico Not Detected
- Make sure to hold BOOTSEL when plugging in
- Check `lsusb` output to verify the Pico is detected
- Try different USB cables/ports

## Next Steps

Once you have this basic setup working, you can:
- Add sensors and actuators
- Implement communication protocols (SPI, I2C, UART)
- Use Embassy's networking features with WiFi chips
- Create more complex async applications
- Explore Embassy's hardware abstraction layer (HAL)

## Resources

- [Embassy Documentation](https://embassy.dev/)
- [Raspberry Pi Pico Documentation](https://www.raspberrypi.org/documentation/microcontrollers/)
- [Rust Embedded Book](https://doc.rust-lang.org/stable/embedded-book/)
- [Embassy Examples](https://github.com/embassy-rs/embassy/tree/main/examples/rp)
