//! # display_demo: async port of the Arduino `Display` sketch (M5Stack PaperDIY)
//!
//! Draws the same screen as `~/Src/Arduino/M5PaperDIY/Display`: a triple black border, four
//! centered lines of text and a bar of four cells (black, dark gray, light gray, black), with
//! `RefreshMode::M5Quality`. The blocking twin is `display_demo` in `rust-m5paperdiy-examples`. Here `epdpi` runs in async
//! mode on the `esp-rtos` Embassy executor and a row waits for the DMA instead of spinning.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example display_demo
//! ```
//!
//! ## Expected
//!
//! - The log prints `refresh done in N ms`, about 1.1 s like the blocking example.
//! - Portrait, 540 by 960, like the sketch (the buffer is turned with `Rotation::Deg270`). White
//!   screen, three black borders 2 px apart, `PAPERDIY`, `Panel: ED047TC1`,
//!   `Resolution: 540 x 960`, `16-Level Grayscale E-Paper`, then a four cell bar.
//! - Differences from the Arduino sketch: `u8g2-fonts` Free Universal Bold instead of
//!   FreeSansBold, close but not the same glyphs. The panel line says `ED047TC1`, the real
//!   panel; the sketch prints `AXE544127`, which is the board's connector. A last line,
//!   `Rust: epdpi async`, says which build a photo shows; the sketch has none.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::{
    pixelcolor::Gray4,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use esp_hal::delay::Delay;
use esp_println as _;
use rust_m5paperdiy_embassy_examples::{epdpi::prelude::*, pmic};
use u8g2_fonts::{
    FontRenderer, fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
};

esp_bootloader_esp_idf::esp_app_desc!();

/// The four bar cells: `TFT_BLACK`, `TFT_DARKGREY`, `TFT_LIGHTGREY`, `TFT_BLACK`.
const BAR_COLORS: [Gray4; 4] = [Gray4::new(0), Gray4::new(7), Gray4::new(13), Gray4::new(0)];

fn draw_demo<D: DrawTarget<Color = Gray4> + OriginDimensions>(fb: &mut D) {
    let Size {
        width: w,
        height: h,
    } = fb.size();
    let _ = fb.clear(Gray4::WHITE);

    // Three 1 px borders, inset 0, 2 and 4 px.
    let line = PrimitiveStyle::with_stroke(Gray4::BLACK, 1);
    for inset in [0u32, 2, 4] {
        let _ = Rectangle::new(
            Point::new(inset as i32, inset as i32),
            Size::new(w - 2 * inset, h - 2 * inset),
        )
        .into_styled(line)
        .draw(fb);
    }

    // Middle-center text, like `setTextDatum(middle_center)`. fub35 and fub25 stand in for
    // FreeSansBold24pt and FreeSansBold18pt.
    let big = FontRenderer::new::<fonts::u8g2_font_fub35_tf>();
    let small = FontRenderer::new::<fonts::u8g2_font_fub25_tf>();
    let centered = |font: &FontRenderer, y: i32, text: core::fmt::Arguments, fb: &mut D| {
        let _ = font.render_aligned(
            text,
            Point::new(w as i32 / 2, y),
            VerticalPosition::Center,
            HorizontalAlignment::Center,
            FontColor::Transparent(Gray4::BLACK),
            fb,
        );
    };
    centered(&big, 115, format_args!("PAPERDIY"), fb);
    centered(&small, 205, format_args!("Panel: ED047TC1"), fb);
    centered(&small, 255, format_args!("Resolution: {} x {}", w, h), fb);
    centered(&small, 305, format_args!("16-Level Grayscale E-Paper"), fb);

    // The bar: from x = 120, `w - 240` wide, four cells, the last takes the remainder.
    let (bar_x, bar_y, bar_w, bar_h) = (120u32, 370u32, w - 240, 58u32);
    let cell_w = bar_w / 4;
    for (i, color) in BAR_COLORS.iter().enumerate() {
        let x = bar_x + i as u32 * cell_w;
        let cw = if i == 3 { bar_x + bar_w - x } else { cell_w };
        let cell = Rectangle::new(Point::new(x as i32, bar_y as i32), Size::new(cw, bar_h));
        let _ = cell.into_styled(PrimitiveStyle::with_fill(*color)).draw(fb);
        let _ = cell.into_styled(line).draw(fb);
    }

    // Not in the sketch: says which build a photo shows.
    centered(
        &FontRenderer::new::<fonts::u8g2_font_fub17_tf>(),
        h as i32 - 40,
        format_args!("Rust: epdpi async"),
        fb,
    );
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_m5paperdiy_embassy_examples::init!();
    rust_m5paperdiy_embassy_examples::start_rtos!(p);
    let mut i2c = rust_m5paperdiy_embassy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let bus = rust_m5paperdiy_embassy_examples::lcd_bus!(p, 16);
    // M5GFX assumes every pixel starts at mid gray (0x88) and has no hard clear.
    let mut prev = vec![0x88u8; Ed047tc1::FB_BYTES];
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let (Ok(mut display), Ok(mut fb)) = (
        EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line),
        EpdFrameBuffer::<Ed047tc1>::new(&mut fb_mem),
    ) else {
        error!("buffer allocation too small");
        rust_m5paperdiy_embassy_examples::halt();
    };

    fb.set_rotation(Rotation::Deg270);
    draw_demo(&mut fb);

    // As the M5GFX sketch does: a white background through the same waveform first.
    let mut white_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    if let Ok(white) = EpdFrameBuffer::<Ed047tc1>::new(&mut white_mem)
        && display
            .refresh(&white, RefreshMode::M5Quality)
            .await
            .is_err()
    {
        error!("white refresh failed");
    }

    let start = Instant::now();
    match display.refresh(&fb, RefreshMode::M5Quality).await {
        Ok(()) => info!("refresh done in {} ms", start.elapsed().as_millis()),
        Err(_) => error!("refresh failed"),
    }
    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}
