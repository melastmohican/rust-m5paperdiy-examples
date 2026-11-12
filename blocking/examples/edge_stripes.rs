//! # edge_stripes: bars at both edges of a row, to see the extra LCD_CAM clock (M5Stack PaperDIY)
//!
//! Backlog `lcd-line-start-check` (`epdpi`), stage 2. `lcd_clock_count` found one more CL cycle than
//! bytes in every row of `LcdBus`. If that clock comes before the data, the picture sits 4 pixels to the
//! right of where the code puts it: x = 0 to 3 are never driven in the gray passes and x = 956 to 959
//! fall off the end of the row. White screen, black marks (y is the row):
//!
//! - coarse bars 4 px wide, 4 px apart, 340 rows tall (y 100 to 439), visible in a photo of the whole panel:
//!   left group x = 0..3, 8..11, 16..19, 24..27, control group x = 472..475, 480..483, 488..491, 496..499,
//!   right group x = 932..935, 940..943, 948..951, 956..959
//! - fine stripes 1 px wide, 1 px apart, 24 rows tall (y 446 to 469), for a close photo: left x = 0, 2, 4, 6,
//!   control x = 478, 480, 482, 484, right x = 953, 955, 957, 959
//!
//! Drawn with `RefreshMode::Quality16` over `LcdBus`, after the clear it does itself.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example edge_stripes
//! ```
//!
//! ## Expected
//!
//! The log prints `refresh done in N ms`. Without the extra clock before the data: in each group 4 bars with
//! the first left bar and the last right bar flush with the glass edge. With it: a 4 px white margin at both
//! edges (the left group starts at x = 4, the last right bar is lost, so the right group shows 3 bars ending
//! 4 px short of the edge). The control group is 4 equal bars either way.
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
use esp_hal::delay::Delay;
use esp_hal::time::Instant;
use esp_println as _;
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

esp_bootloader_esp_idf::esp_app_desc!();

const BAR_Y: i32 = 100;
const BAR_H: u32 = 340;
const STRIPE_Y: i32 = 446;
const STRIPE_H: u32 = 24;

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let delay = Delay::new();
    let mut pmic_delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut pmic_delay);

    let bus = rust_m5paperdiy_examples::lcd_bus!(p, 16);
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

    let black = PrimitiveStyle::with_fill(Gray4::BLACK);
    let mut mark = |x: i32, w: u32, y: i32, h: u32| {
        let _ = Rectangle::new(Point::new(x, y), Size::new(w, h))
            .into_styled(black)
            .draw(&mut fb);
    };
    for x in [0, 8, 16, 24, 472, 480, 488, 496, 932, 940, 948, 956] {
        mark(x, 4, BAR_Y, BAR_H);
    }
    for x in [0, 2, 4, 6, 478, 480, 482, 484, 953, 955, 957, 959] {
        mark(x, 1, STRIPE_Y, STRIPE_H);
    }
    let style = MonoTextStyle::new(&FONT_10X20, Gray4::BLACK);
    let _ = Text::new(
        "Rust: edge stripes, Quality16 LcdBus",
        Point::new(40, 500),
        style,
    )
    .draw(&mut fb);

    let start = Instant::now();
    match display.refresh(&fb, RefreshMode::Quality16) {
        Ok(()) => info!("refresh done in {} ms", start.elapsed().as_millis()),
        Err(_) => error!("refresh failed"),
    }
    loop {
        delay.delay_millis(1000);
    }
}
