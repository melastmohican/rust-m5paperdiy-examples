//! # epdpi_window: partial-window updates (M5Stack PaperDIY)
//!
//! Library diagnostic. Shows a 16-level gray ramp with `RefreshMode::M5Quality` and a white box
//! with a black border in the middle of it, then updates a counter inside the box, five times
//! with `M5Fast` and five times with `M5Text`, through `EpdDisplay::refresh_dirty`. The
//! framebuffer tracks what was drawn, so only the inside of the box is refreshed.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_window
//! ```
//!
//! ## Expected
//!
//! - The ramp and the empty box appear first (about 1 to 2 s), then the inside of the box starts
//!   changing every 2 s. The black border stays.
//! - **The ramp around the box must not flash or change while the counter updates.** That is
//!   the point of a window: everything outside it holds. Watch the bars next to the box.
//! - The log prints `window update N in X ms`. Every row is still clocked, so a window update
//!   costs about the same as a full update with the same mode (M5GFX and FastEPD do the same).
//! - The box reads `Count: N` and the mode name, `M5Fast` for the first five updates, then
//!   `M5Text`.
#![no_std]
#![no_main]

use alloc::vec;
use core::fmt::Write;
use defmt::{error, info};
use embedded_graphics::{
    mono_font::MonoTextStyle,
    pixelcolor::Gray4,
    prelude::*,
    primitives::{PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, StrokeAlignment},
    text::Text,
};
use esp_hal::delay::Delay;
use esp_hal::time::Instant;
use esp_println as _;
use profont::PROFONT_24_POINT;
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

/// The box in the middle of the ramp, 400 by 160 pixels.
const BOX: Rectangle = Rectangle::new(Point::new(280, 160), Size::new(400, 160));
/// What is updated: the box without its 4 pixel border.
const INSIDE: Rectangle = Rectangle::new(Point::new(284, 164), Size::new(392, 152));

/// Tiny fixed-capacity string for the counter text.
struct Label {
    buf: [u8; 40],
    len: usize,
}

impl Write for Label {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let end = self.len + bytes.len();
        let dst = self.buf.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        dst.copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let bus = rust_m5paperdiy_examples::lcd_bus!(p, 16);
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

    if display.clear().is_err() {
        error!("initial clear failed");
    }

    // The ramp, 16 bars 60 px wide and 480 tall, the empty box in the middle and a label under it.
    for level in 0..16u8 {
        let _ = Rectangle::new(Point::new(i32::from(level) * 60, 0), Size::new(60, 480))
            .into_styled(PrimitiveStyle::with_fill(Gray4::new(level)))
            .draw(&mut fb);
    }
    let bordered = PrimitiveStyleBuilder::new()
        .fill_color(Gray4::WHITE)
        .stroke_color(Gray4::BLACK)
        .stroke_width(4)
        .stroke_alignment(StrokeAlignment::Inside)
        .build();
    let _ = BOX.into_styled(bordered).draw(&mut fb);
    let style = MonoTextStyle::new(&PROFONT_24_POINT, Gray4::BLACK);
    let _ = Text::new("Rust: epdpi window", Point::new(40, 520), style).draw(&mut fb);
    let start = Instant::now();
    match display.refresh_dirty(&mut fb, RefreshMode::M5Quality) {
        Ok(()) => info!("ramp done in {} ms", start.elapsed().as_millis()),
        Err(_) => error!("ramp failed"),
    }

    for count in 0..10u32 {
        let (mode, name) = if count < 5 {
            (RefreshMode::M5Fast, "M5Fast")
        } else {
            (RefreshMode::M5Text, "M5Text")
        };
        // Wipe the inside of the box, then draw the counter. Only the inside becomes dirty.
        let _ = INSIDE
            .into_styled(PrimitiveStyle::with_fill(Gray4::WHITE))
            .draw(&mut fb);
        let mut label = Label {
            buf: [0; 40],
            len: 0,
        };
        let _ = write!(label, "Count: {}", count);
        if let Ok(text) = core::str::from_utf8(&label.buf[..label.len]) {
            let _ = Text::new(text, Point::new(310, 225), style).draw(&mut fb);
        }
        let _ = Text::new(name, Point::new(310, 275), style).draw(&mut fb);
        let start = Instant::now();
        match display.refresh_dirty(&mut fb, mode) {
            Ok(()) => info!(
                "window update {} ({}) in {} ms",
                count,
                name,
                start.elapsed().as_millis()
            ),
            Err(_) => error!("window update failed"),
        }
        delay.delay_millis(2000);
    }
    info!("all window updates done");
    loop {
        delay.delay_millis(1000);
    }
}
