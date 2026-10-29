//! # display_demo: port of the Arduino `Display` sketch (M5Stack PaperDIY)
//!
//! Draws the same screen as `~/Src/Arduino/M5PaperDIY/Display`: a triple black border, four
//! centered lines of text and a bar of four cells (black, dark gray, light gray, black), with
//! `RefreshMode::M5Quality`. The async twin is `display_demo` in `rust-m5paperdiy-embassy-examples`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example display_demo
//! ```
//!
//! ## Expected
//!
//! - The log prints `refresh done in N ms`, about 1 s.
//! - White screen, three black borders 2 px apart, `PAPERDIY`, `Panel: ED047TC1`,
//!   `Resolution: 960 x 540`, `16-Level Grayscale E-Paper`, then a four cell bar.
//! - Differences from the Arduino sketch: one monospace font (`profont` 24 point) for every
//!   line, no bold and no larger heading. The panel line says `ED047TC1`, the real panel; the
//!   sketch prints `AXE544127`, which is the board's connector.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embedded_graphics::{
    mono_font::MonoTextStyle,
    pixelcolor::Gray4,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use esp_hal::delay::Delay;
use esp_hal::time::Instant;
use esp_println as _;
use profont::PROFONT_24_POINT;
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

esp_bootloader_esp_idf::esp_app_desc!();

const W: u32 = 960;
const H: u32 = 540;

/// The four bar cells: `TFT_BLACK`, `TFT_DARKGREY`, `TFT_LIGHTGREY`, `TFT_BLACK`.
const BAR_COLORS: [Gray4; 4] = [Gray4::new(0), Gray4::new(7), Gray4::new(13), Gray4::new(0)];

fn draw_demo<D: DrawTarget<Color = Gray4>>(fb: &mut D) {
    let _ = fb.clear(Gray4::WHITE);

    // Three 1 px borders, inset 0, 2 and 4 px.
    let line = PrimitiveStyle::with_stroke(Gray4::BLACK, 1);
    for inset in [0u32, 2, 4] {
        let _ = Rectangle::new(
            Point::new(inset as i32, inset as i32),
            Size::new(W - 2 * inset, H - 2 * inset),
        )
        .into_styled(line)
        .draw(fb);
    }

    // Middle-center text, like `setTextDatum(middle_center)`.
    let font = MonoTextStyle::new(&PROFONT_24_POINT, Gray4::BLACK);
    let centered = TextStyleBuilder::new()
        .alignment(Alignment::Center)
        .baseline(Baseline::Middle)
        .build();
    for (text, y) in [
        ("PAPERDIY", 115),
        ("Panel: ED047TC1", 205),
        ("Resolution: 960 x 540", 255),
        ("16-Level Grayscale E-Paper", 305),
    ] {
        let _ = Text::with_text_style(text, Point::new(W as i32 / 2, y), font, centered).draw(fb);
    }

    // The bar: 720 wide from x = 120, four cells, the last takes the remainder.
    let (bar_x, bar_y, bar_w, bar_h) = (120u32, 370u32, W - 240, 58u32);
    let cell_w = bar_w / 4;
    for (i, color) in BAR_COLORS.iter().enumerate() {
        let x = bar_x + i as u32 * cell_w;
        let w = if i == 3 { bar_x + bar_w - x } else { cell_w };
        let cell = Rectangle::new(Point::new(x as i32, bar_y as i32), Size::new(w, bar_h));
        let _ = cell.into_styled(PrimitiveStyle::with_fill(*color)).draw(fb);
        let _ = cell.into_styled(line).draw(fb);
    }
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let bus = rust_m5paperdiy_examples::lcd_bus!(p, 16);
    // M5GFX assumes every pixel starts at mid gray (0x88) and has no hard clear.
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

    draw_demo(&mut fb);

    // As the M5GFX sketch does: a white background through the same waveform first.
    let mut white_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    if let Ok(white) = EpdFrameBuffer::<Ed047tc1>::new(&mut white_mem)
        && display.refresh(&white, RefreshMode::M5Quality).is_err()
    {
        error!("white refresh failed");
    }

    let start = Instant::now();
    match display.refresh(&fb, RefreshMode::M5Quality) {
        Ok(()) => info!("refresh done in {} ms", start.elapsed().as_millis()),
        Err(_) => error!("refresh failed"),
    }
    loop {
        delay.delay_millis(1000);
    }
}
