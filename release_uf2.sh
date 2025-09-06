#!/bin/env sh

cargo build --release && elf2uf2-rs target/thumbv6m-none-eabi/release/pico-climate target/thumbv6m-none-eabi/release/pico-climate.uf2
