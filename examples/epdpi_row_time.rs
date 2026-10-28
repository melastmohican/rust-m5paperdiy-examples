//! # epdpi_row_time: how long a row takes (M5Stack PaperDIY)
//!
//! Library diagnostic. Measures, in microseconds per row, what the `FastBus` data phase costs,
//! what building one scanline costs, and what a whole pass costs. M5GFX clocks a row's data
//! in about 15.5 us (248 bytes at 16 MHz), and the 16-gray waveforms assume a time near that.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_row_time
//! ```
//!
//! The numbers are printed as plain text, so read them with a serial monitor.
//!
//! ## Expected
//!
//! One line per measurement. The bus line shows how far `FastBus` is from 15.5 us per row.
#![no_std]
#![no_main]

use alloc::vec;
use esp_hal::delay::Delay;
use esp_hal::time::Instant;
use esp_println::println;
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

const ROWS: u32 = 540;

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let mut bus = rust_m5paperdiy_examples::display_pins!(p).into_fast_bus();
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let fb = vec![0x37u8; Ed047tc1::FB_STRIDE_BYTES];

    // Scanline of Hold codes: the panel is not driven, only the timing is measured.
    WaveformEngine::uniform_scanline::<Ed047tc1>(DriveCode::Hold, &mut line);

    let _ = bus.set_power(true);
    let _ = bus.start_frame();
    let start = Instant::now();
    for _ in 0..ROWS {
        let _ = bus.write_scanline(&line);
        let _ = bus.next_row();
    }
    let bus_us = start.elapsed().as_micros();
    let _ = bus.end_frame();

    let start = Instant::now();
    for _ in 0..ROWS {
        WaveformEngine::lut_scanline::<Ed047tc1>(
            &fb,
            Ed047tc1::QUALITY_LUT,
            Ed047tc1::QUALITY_PASSES,
            3,
            &mut line,
        );
    }
    let build_us = start.elapsed().as_micros();

    let _ = bus.start_frame();
    let start = Instant::now();
    for _ in 0..ROWS {
        WaveformEngine::lut_scanline::<Ed047tc1>(
            &fb,
            Ed047tc1::QUALITY_LUT,
            Ed047tc1::QUALITY_PASSES,
            3,
            &mut line,
        );
        let _ = bus.write_scanline(&line);
        let _ = bus.next_row();
    }
    let pass_us = start.elapsed().as_micros();
    let _ = bus.end_frame();
    let _ = bus.set_power(false);

    loop {
        println!(
            "row time: bus write+next_row {} us, build scanline {} us, whole row {} us (M5GFX data phase 15.5 us)",
            bus_us / u64::from(ROWS),
            build_us / u64::from(ROWS),
            pass_us / u64::from(ROWS),
        );
        delay.delay_millis(1000);
    }
}
