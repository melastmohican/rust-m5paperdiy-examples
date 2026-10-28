//! # epdpi_fast_mono: differential updates (M5Stack PaperDIY)
//!
//! Library diagnostic, step 6 of 6.
//!
//! Counts up on screen with `RefreshMode::FastMono`. That mode drives only the pixels that
//! changed since the last refresh, in 4 passes plus a discharge pass, with no full-screen
//! flash. Pixels at gray level 8 or above count as white. Every 10 updates the example runs
//! a full `clear` to remove the ghosting that differential updates leave behind.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_fast_mono
//! ```
//!
//! ## Expected
//!
//! - The log prints `update N in 379 ms`. Every update takes the same time, because the
//!   number of passes is fixed. It does not depend on how many pixels changed.
//! - Only the digits change on screen, with no black or white flash, until the periodic
//!   clear at update 10.
//! - Old digits may fade slightly between clears.
//! - A second line reads `Rust: epdpi FastMono`. The Arduino reference sketch
//!   `EpdFastCounter` prints `Arduino: M5GFX epd_fast`, so a filmed clip says which build it is.
//!
//! ## Limits
//!
//! `FastMono` works on the whole frame. There is no partial-window refresh yet, so a tiny
//! change still costs 379 ms.
#![no_std]
#![no_main]

use alloc::vec;
use core::fmt::Write;
use defmt::{error, info};
use embedded_graphics::{mono_font::MonoTextStyle, pixelcolor::Gray4, prelude::*, text::Text};
use esp_hal::delay::Delay;
use esp_hal::time::Instant;
use esp_println as _;
use profont::PROFONT_24_POINT;
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

    let style = MonoTextStyle::new(&PROFONT_24_POINT, Gray4::BLACK);
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
        let _ = Text::new("Rust: epdpi FastMono", Point::new(40, 130), style).draw(&mut fb);

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
