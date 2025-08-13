#!/bin/bash

set -e

echo "Setting up Raspberry Pi Pico Embassy development environment..."

# Create project directory structure
mkdir -p src .cargo

# Install additional Rust components inside container if not already installed
echo "Installing Rust components..."
rustup target add thumbv6m-none-eabi
cargo install flip-link

echo "Building the project..."
cargo build --release

echo ""
echo "Setup complete! Here's how to use your development environment:"
echo ""
echo "To flash your Pico:"
echo "1. Hold BOOTSEL button on Pico and plug it into USB"
echo "2. Run: cargo run --release"
echo ""
echo "To use with a debug probe:"
echo "1. Connect your debug probe to the Pico"
echo "2. Edit .cargo/config.toml to use probe-rs runner"
echo "3. Run: cargo run --release"
echo ""
echo "To view debug output:"
echo "- Debug messages will appear in the terminal when using probe-rs"
echo "- Or use: probe-rs attach --chip RP2040"
