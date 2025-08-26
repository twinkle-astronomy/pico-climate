FROM rust:1.88-bookworm

# Install required system packages
RUN apt-get update && apt-get install -y \
    git \
    curl \
    build-essential \
    pkg-config \
    libudev-dev \
    libusb-1.0-0-dev \
    && rm -rf /var/lib/apt/lists/*

# Install ARM Cortex-M target
RUN rustup target add thumbv6m-none-eabi
RUN rustup target add thumbv8m.main-none-eabihf


# Install additional components
RUN rustup component add llvm-tools-preview
RUN rustup component add rustfmt

# Set working directory
WORKDIR /app

# Create a non-root user for development
RUN useradd -m -s /bin/bash developer && \
    chown -R developer:developer /app

USER developer

RUN cargo install \
    cargo-binutils \
    elf2uf2-rs \
    probe-rs-tools \
    flip-link
