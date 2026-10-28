//! # Text and Shapes (M5Stack PaperDIY)
//!
//! Step 5. Draws a frame, a circle and some text with `embedded-graphics`, then refreshes
//! with `RefreshMode::Quality16`.
//!
//! ## What to watch
//!
//! - Text reads left to right and is not mirrored. Mirrored or rotated output means the
//!   panel is mounted in a different orientation than the driver assumes, which is a
//!   framebuffer mapping change, not a wiring fault.
//! - The border touches all four edges. A missing edge means rows or columns are clipped.
#![no_std]
#![no_main]

use alloc::vec;
use defmt::{error, info};
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Gray4,
    prelude::*,
    primitives::{Circle, PrimitiveStyle, Rectangle},
    text::Text,
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

    let bus = rust_m5paperdiy_examples::display_pins!(p).into_bus();
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

    let black = Gray4::BLACK;
    let _ = Rectangle::new(Point::zero(), Size::new(960, 540))
        .into_styled(PrimitiveStyle::with_stroke(black, 4))
        .draw(&mut fb);
    let _ = Circle::new(Point::new(700, 120), 160)
        .into_styled(PrimitiveStyle::with_fill(Gray4::new(6)))
        .draw(&mut fb);
    let style = MonoTextStyle::new(&FONT_10X20, black);
    let _ = Text::new("epdpi on M5Stack PaperDIY", Point::new(40, 80), style).draw(&mut fb);
    let _ = Text::new("ED047TC1 960x540, 16 grays", Point::new(40, 120), style).draw(&mut fb);

    let start = Instant::now();
    match display.refresh(&fb, RefreshMode::Quality16) {
        Ok(()) => info!("refresh done in {} ms", start.elapsed().as_millis()),
        Err(_) => error!("refresh failed"),
    }
    loop {
        delay.delay_millis(1000);
    }
}
