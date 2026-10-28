//! # epdpi_gray_ramp: 16-level grayscale (M5Stack PaperDIY)
//!
//! Library diagnostic, step 4 of 6. First test of the waveform engine.
//!
//! Draws 16 vertical bars, 60 pixels wide, gray level 0 (black) on the left to 15 (white)
//! on the right, and shows them with `RefreshMode::Quality16`. That mode clears the panel
//! (8 darken, 8 lighten passes), then runs all 8 passes of the ED047TC1 waveform matrix.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_gray_ramp
//! ```
//!
//! ## Expected
//!
//! - The log prints `refresh done in N ms`. About 1,100 ms with `FastBus`.
//! - 16 bars that lighten steadily from left to right.
//!
//! ## Why this example uses `FastBus`
//!
//! The waveform matrix assumes a row time near M5GFX's 16 microseconds. The portable
//! `GenericGpioBus` takes about 250 microseconds per row, so every pass overdrives the
//! pixels. On the hardware this was developed on, the result was dark steps on the left and
//! one cream-white block from level 8 up. `FastBus` writes the data pins through the GPIO
//! set and clear registers and restores the intended steps.
//!
//! ## Known limits
//!
//! The top levels (about 12 to 15) look alike to a phone camera and may not be separable by
//! eye either. One band near level 10 can look slightly darker. Neither has been traced to
//! the waveform table yet.
#![no_std]
#![no_main]

use alloc::vec;
use defmt::{error, info};
use embedded_graphics::{
    pixelcolor::Gray4,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use esp_hal::delay::Delay;
use esp_hal::time::Instant;
use esp_println as _;
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let bus = rust_m5paperdiy_examples::display_pins!(p).into_fast_bus();
    let mut prev = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let (Ok(mut display), Ok(mut fb)) = (
        EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line),
        EpdFrameBuffer::<Ed047tc1>::new(&mut fb_mem),
    ) else {
        error!("buffer allocation too small");
        rust_m5paperdiy_examples::halt();
    };

    for level in 0..16u8 {
        let _ = Rectangle::new(Point::new(i32::from(level) * 60, 0), Size::new(60, 540))
            .into_styled(PrimitiveStyle::with_fill(Gray4::new(level)))
            .draw(&mut fb);
    }

    let start = Instant::now();
    match display.refresh(&fb, RefreshMode::Quality16) {
        Ok(()) => info!("refresh done in {} ms", start.elapsed().as_millis()),
        Err(_) => error!("refresh failed"),
    }
    loop {
        delay.delay_millis(1000);
    }
}
