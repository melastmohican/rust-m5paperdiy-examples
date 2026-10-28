//! # Fast Mono Updates (M5Stack PaperDIY)
//!
//! Step 6. Counts up on screen with `RefreshMode::FastMono`, which only drives pixels that
//! changed, so there is no full-screen flash. Every 10 updates it runs a full `clear` to
//! wipe the ghosting that differential updates leave behind.
//!
//! ## What to watch
//!
//! - Each update takes well under the `Quality16` time and does not flash.
//! - Old digits fade slightly under repeated updates, then vanish after the periodic clear.
#![no_std]
#![no_main]

use alloc::vec;
use core::fmt::Write;
use defmt::{error, info};
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Gray4,
    prelude::*,
    text::Text,
};
use esp_hal::delay::Delay;
use esp_hal::time::Instant;
use esp_println as _;
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

/// Tiny fixed-capacity string for the counter text.
struct Label {
    buf: [u8; 32],
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

    if display.clear().is_err() {
        error!("initial clear failed");
    }

    let style = MonoTextStyle::new(&FONT_10X20, Gray4::BLACK);
    let mut count = 0u32;
    loop {
        fb.fill(0xF);
        let mut label = Label {
            buf: [0; 32],
            len: 0,
        };
        let _ = write!(label, "Count: {}", count);
        if let Ok(text) = core::str::from_utf8(&label.buf[..label.len]) {
            let _ = Text::new(text, Point::new(40, 80), style).draw(&mut fb);
        }

        let start = Instant::now();
        match display.refresh(&fb, RefreshMode::FastMono) {
            Ok(()) => info!("update {} in {} ms", count, start.elapsed().as_millis()),
            Err(_) => error!("update failed"),
        }

        count += 1;
        if count.is_multiple_of(10) && display.clear().is_err() {
            error!("periodic clear failed");
        }
        delay.delay_millis(2000);
    }
}
