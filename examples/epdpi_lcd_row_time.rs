//! # epdpi_lcd_row_time: how long a row takes on the LCD_CAM bus (M5Stack PaperDIY)
//!
//! Library diagnostic. Like `epdpi_row_time`, but for `lcd_bus::LcdBus`. Prints microseconds
//! per row for the bus alone and for a whole row with the scanline built. M5GFX clocks a row
//! in about 15.5 us.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_lcd_row_time
//! ```
//!
//! The numbers are plain text, read them with a serial monitor.
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

    let mut bus = rust_m5paperdiy_examples::lcd_bus!(p, 16);
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let fb = vec![0x37u8; Ed047tc1::FB_STRIDE_BYTES];
    WaveformEngine::uniform_scanline::<Ed047tc1>(DriveCode::Hold, &mut line);

    let _ = bus.set_power(true);
    let _ = bus.start_frame();
    let start = Instant::now();
    let mut errors = 0u32;
    for _ in 0..ROWS {
        errors += u32::from(bus.write_scanline(&line).is_err());
        let _ = bus.next_row();
    }
    let bus_us = start.elapsed().as_micros();
    let _ = bus.end_frame();

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
        errors += u32::from(bus.write_scanline(&line).is_err());
        let _ = bus.next_row();
    }
    let row_us = start.elapsed().as_micros();
    let _ = bus.end_frame();
    let _ = bus.set_power(false);

    loop {
        println!(
            "lcd row time: bus {} us per row, whole row {} us, errors {} (M5GFX data phase 15.5 us)",
            bus_us / u64::from(ROWS),
            row_us / u64::from(ROWS),
            errors,
        );
        delay.delay_millis(1000);
    }
}
