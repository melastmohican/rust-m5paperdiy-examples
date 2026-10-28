//! # Hello M5PaperDIY
//!
//! The smallest real program for the M5Stack PaperDIY: it draws "Hello M5PaperDIY!" in the
//! middle of the screen with `epdpi`, then stops. `cargo run --release` flashes it.
//!
//! The text is the 10x20 `embedded-graphics` font drawn at 5 times scale through a small
//! pixel-doubling adapter, so each glyph is 50 by 100 pixels. That is readable across a room,
//! unlike the font at its native size.
//!
//! What it does, in order:
//! 1. Maps the 8 MB PSRAM into the heap (`init!`).
//! 2. Switches the panel rail on through the M5PM1 PMIC.
//! 3. Clears the panel and shows the text with `RefreshMode::Quality16`.
//!
//! Expect the screen to flash black then white, then settle on the text after about a
//! second. The log prints `shown in N ms`. If nothing happens, run the `epdpi_*`
//! diagnostics in the order listed in the README.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;

use defmt::{error, info};
use embedded_graphics::{
    Pixel,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Gray4,
    prelude::*,
    primitives::Rectangle,
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use esp_hal::delay::Delay;
use esp_hal::time::Instant;
use esp_println as _;
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

esp_bootloader_esp_idf::esp_app_desc!();

/// Draws every source pixel as a `factor` by `factor` block on the wrapped target.
struct Scaled<'a, D> {
    inner: &'a mut D,
    factor: u32,
}

impl<D: OriginDimensions> OriginDimensions for Scaled<'_, D> {
    fn size(&self) -> Size {
        let s = self.inner.size();
        Size::new(s.width / self.factor, s.height / self.factor)
    }
}

impl<D: DrawTarget<Color = Gray4> + OriginDimensions> DrawTarget for Scaled<'_, D> {
    type Color = Gray4;
    type Error = D::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Gray4>>,
    {
        let f = self.factor;
        for Pixel(p, color) in pixels {
            let block = Rectangle::new(p * f as i32, Size::new(f, f));
            self.inner.fill_solid(&block, color)?;
        }
        Ok(())
    }
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    if pmic::epd_power_on(&mut i2c, &mut delay).is_err() {
        error!("M5PM1 did not answer, the panel rail is off");
    }

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

    let mut scaled = Scaled {
        inner: &mut fb,
        factor: 5,
    };
    let center = Point::new(
        scaled.size().width as i32 / 2,
        scaled.size().height as i32 / 2,
    );
    let style = MonoTextStyle::new(&FONT_10X20, Gray4::BLACK);
    let layout = TextStyleBuilder::new()
        .alignment(Alignment::Center)
        .baseline(Baseline::Middle)
        .build();
    let _ = Text::with_text_style("Hello M5PaperDIY!", center, style, layout).draw(&mut scaled);

    let start = Instant::now();
    match display.refresh(&fb, RefreshMode::Quality16) {
        Ok(()) => info!("shown in {} ms", start.elapsed().as_millis()),
        Err(_) => error!("refresh failed"),
    }
    rust_m5paperdiy_examples::halt();
}
