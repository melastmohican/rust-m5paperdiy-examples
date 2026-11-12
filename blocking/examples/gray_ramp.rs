//! # gray_ramp: one 16-level ramp, mode and bus chosen at build time (M5Stack PaperDIY)
//!
//! The photo set of the FastEPD comparison (`epdpi` backlog `fastepd-ramp-photo`). Sixteen bars,
//! 60 px wide and 480 tall, a label line below, level 0 (black) on the left to 15 (white) on
//! the right, the layout of the Arduino sketches `EpdQualityRamp` (M5GFX) and
//! `FastEPD/grayscale_ramp`.
//!
//! ## Run
//!
//! Pick one with the `RAMP` environment variable (default `q16-lcd`):
//!
//! ```text
//! RAMP=q16-lcd  cargo run --release --example gray_ramp   # Quality16 over LcdBus
//! RAMP=q16-fast cargo run --release --example gray_ramp   # Quality16 over FastBus
//! RAMP=m5-lcd   cargo run --release --example gray_ramp   # M5Quality over LcdBus
//! ```
//!
//! ## Expected
//!
//! - The log prints `ramp <name>: refresh done in N ms`.
//! - 16 bars from black to white, 17 bold tick marks under them at the bar edges and a label naming the
//!   build, for the photo. `M5Quality` draws a white refresh first, like the M5GFX sketch.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Gray4,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use epdpi::EpdParallelBus;
use esp_hal::delay::Delay;
use esp_hal::time::Instant;
use esp_println as _;
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

esp_bootloader_esp_idf::esp_app_desc!();

fn show<B: EpdParallelBus>(bus: B, mode: RefreshMode, name: &str, label: &str) -> ! {
    let delay = Delay::new();
    let mut prev = vec![0x88u8; Ed047tc1::FB_BYTES];
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
        let _ = Rectangle::new(Point::new(i32::from(level) * 60, 0), Size::new(60, 480))
            .into_styled(PrimitiveStyle::with_fill(Gray4::new(level)))
            .draw(&mut fb);
    }
    // Tick marks under the bars, 6 px wide and 16 tall, centred on the bar edges (clamped at the
    // panel edges), so the photo script finds the bars.
    for k in 0..=16i32 {
        let x = (k * 60 - 3).clamp(0, 954);
        let _ = Rectangle::new(Point::new(x, 482), Size::new(6, 16))
            .into_styled(PrimitiveStyle::with_fill(Gray4::BLACK))
            .draw(&mut fb);
    }
    let style = MonoTextStyle::new(&FONT_10X20, Gray4::BLACK);
    let _ = Text::new(label, Point::new(40, 520), style).draw(&mut fb);

    // As the M5GFX sketch does: a white background through the same waveform first. `M5Quality`
    // erases from the record, not from the glass, so a ramp left on it would show through.
    if mode == RefreshMode::M5Quality {
        let mut white_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
        if let Ok(white) = EpdFrameBuffer::<Ed047tc1>::new(&mut white_mem)
            && display.refresh(&white, mode).is_err()
        {
            error!("white refresh failed");
        }
    }

    let start = Instant::now();
    match display.refresh(&fb, mode) {
        Ok(()) => info!(
            "ramp {}: refresh done in {} ms",
            name,
            start.elapsed().as_millis()
        ),
        Err(_) => error!("refresh failed"),
    }
    loop {
        delay.delay_millis(1000);
    }
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    match option_env!("RAMP").unwrap_or("q16-lcd") {
        "q16-fast" => {
            let bus = rust_m5paperdiy_examples::display_pins!(p).into_fast_bus();
            show(
                bus,
                RefreshMode::Quality16,
                "q16-fast",
                "Rust: Quality16 FastBus",
            )
        }
        "m5-lcd" => {
            let bus = rust_m5paperdiy_examples::lcd_bus!(p, 16);
            show(
                bus,
                RefreshMode::M5Quality,
                "m5-lcd",
                "Rust: M5Quality LcdBus",
            )
        }
        _ => {
            let bus = rust_m5paperdiy_examples::lcd_bus!(p, 16);
            show(
                bus,
                RefreshMode::Quality16,
                "q16-lcd",
                "Rust: Quality16 LcdBus",
            )
        }
    }
}
