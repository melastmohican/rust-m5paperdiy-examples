//! # buttons: async port of the Arduino `Button` sketch (M5Stack PaperDIY)
//!
//! Shows `PaperDIY Button Test`, the last action and the pin numbers. Pressing BtnA (GPIO4) or
//! BtnB (GPIO3, both active low) redraws the screen with `BtnA pressed` or `BtnB pressed`,
//! through `RefreshMode::M5Text` like the sketch's `epd_text`. The blocking twin is `buttons` in
//! `rust-m5paperdiy-examples`. Here `epdpi` runs in async mode on the `esp-rtos` Embassy
//! executor.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example buttons
//! ```
//!
//! ## Expected
//!
//! - After the first draw the log prints `ready`. Each press prints `BtnA pressed` or
//!   `BtnB pressed`, then `update in N ms` when the screen is done.
//! - A press during an update is seen after it ends, as in the sketch.
//! - Portrait, 540 by 960, like the sketch (the buffer is turned with `Rotation::Deg270`).
//! - Differences from the Arduino sketch: `u8g2-fonts` Free Universal Bold instead of
//!   FreeSansBold, close but not the same glyphs. A last line, `Rust: epdpi async`, says which build a
//!   photo shows; the sketch has none.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Pull};
use esp_println as _;
use rust_m5paperdiy_embassy_examples::{epdpi::prelude::*, pmic};
use u8g2_fonts::{
    FontRenderer, fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
};

esp_bootloader_esp_idf::esp_app_desc!();

fn draw_status<D: DrawTarget<Color = Gray4> + OriginDimensions>(fb: &mut D, last_action: &str) {
    let (w, h) = (fb.size().width as i32, fb.size().height as i32);
    let _ = fb.clear(Gray4::WHITE);
    // fub35 and fub25 stand in for FreeSansBold24pt and FreeSansBold18pt.
    let big = FontRenderer::new::<fonts::u8g2_font_fub35_tf>();
    let small = FontRenderer::new::<fonts::u8g2_font_fub25_tf>();
    let tiny = FontRenderer::new::<fonts::u8g2_font_fub17_tf>();
    for (font, text, y) in [
        (&big, "PaperDIY Button Test", h / 2 - 100),
        (&small, "Last action:", h / 2 - 20),
        (&small, last_action, h / 2 + 50),
        (&small, "BtnA: GPIO4   BtnB: GPIO3", h / 2 + 135),
        // Not in the sketch: says which build a photo shows.
        (&tiny, "Rust: epdpi async", h - 40),
    ] {
        let _ = font.render_aligned(
            text,
            Point::new(w / 2, y),
            VerticalPosition::Center,
            HorizontalAlignment::Center,
            FontColor::Transparent(Gray4::BLACK),
            fb,
        );
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_m5paperdiy_embassy_examples::init!();
    rust_m5paperdiy_embassy_examples::start_rtos!(p);
    let mut i2c = rust_m5paperdiy_embassy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let cfg = InputConfig::default().with_pull(Pull::Up);
    let btn_a = Input::new(p.GPIO4, cfg);
    let btn_b = Input::new(p.GPIO3, cfg);

    let bus = rust_m5paperdiy_embassy_examples::lcd_bus!(p, 16);
    let mut prev = vec![0xFFu8; Ed047tc1::FB_BYTES];
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
    if display.clear().await.is_err() {
        error!("initial clear failed");
    }
    draw_status(&mut fb, "Press BtnA or BtnB");
    let start = Instant::now();
    match display.refresh(&fb, RefreshMode::M5Text).await {
        Ok(()) => info!("first draw in {} ms", start.elapsed().as_millis()),
        Err(_) => error!("first draw failed"),
    }
    info!("ready");

    // `wasPressed`: act on the released-to-pressed edge, polled every 20 ms like the sketch.
    let (mut was_a, mut was_b) = (btn_a.is_low(), btn_b.is_low());
    loop {
        let (now_a, now_b) = (btn_a.is_low(), btn_b.is_low());
        let action = if now_a && !was_a {
            Some("BtnA pressed")
        } else if now_b && !was_b {
            Some("BtnB pressed")
        } else {
            None
        };
        if let Some(text) = action {
            info!("{}", text);
            draw_status(&mut fb, text);
            let start = Instant::now();
            match display.refresh(&fb, RefreshMode::M5Text).await {
                Ok(()) => info!("update in {} ms", start.elapsed().as_millis()),
                Err(_) => error!("update failed"),
            }
        }
        // Keep the pre-update state, so a press during the 0.7 s update is an edge next time.
        (was_a, was_b) = (now_a, now_b);
        Timer::after(Duration::from_millis(20)).await;
    }
}
