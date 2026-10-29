//! # epdpi_m5_modes: the M5GFX fast, fastest and text modes (M5Stack PaperDIY)
//!
//! Library diagnostic. Counts up on screen with `RefreshMode::M5Fast`, `M5Fastest` and
//! `M5Text` in turn, five updates each, over `lcd_bus::LcdBus` (the waveforms assume a row
//! near 16 us). A full `clear` runs between modes so each starts from white.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_m5_modes
//! ```
//!
//! ## Expected
//!
//! - The log prints `M5Fast update N in X ms` and the same for `M5Fastest` and `M5Text`. The
//!   passes are fixed, so every update of a mode takes the same time. Passes: fast 8 plus a
//!   discharge pass, fastest 5 plus one, text 2 erase and 12 draw plus one.
//! - M5GFX `epd_fast` took 280 ms per update on this board (`EpdFastCounter` sketch), so
//!   `M5Fast` should land near it. `M5Fastest` should be clearly quicker, `M5Text` slower.
//! - Only the digits change on screen. The second line names the mode.
//! - `M5Fast` and `M5Fastest` flash the changed digits, `M5Text` flashes the digits and the
//!   label but leaves the white background alone.
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

const UPDATES_PER_MODE: u32 = 5;

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

    let style = MonoTextStyle::new(&PROFONT_24_POINT, Gray4::BLACK);
    let modes = [
        (RefreshMode::M5Fast, "M5Fast"),
        (RefreshMode::M5Fastest, "M5Fastest"),
        (RefreshMode::M5Text, "M5Text"),
    ];
    for (mode, name) in modes {
        if display.clear().is_err() {
            error!("clear failed");
        }
        let mut total_ms = 0u64;
        for count in 0..UPDATES_PER_MODE {
            fb.fill(0xF);
            let mut label = Label {
                buf: [0; 32],
                len: 0,
            };
            let _ = write!(label, "Count: {}", count);
            if let Ok(text) = core::str::from_utf8(&label.buf[..label.len]) {
                let _ = Text::new(text, Point::new(40, 80), style).draw(&mut fb);
            }
            let mut tag = Label {
                buf: [0; 32],
                len: 0,
            };
            let _ = write!(tag, "Rust: epdpi {}", name);
            if let Ok(text) = core::str::from_utf8(&tag.buf[..tag.len]) {
                let _ = Text::new(text, Point::new(40, 130), style).draw(&mut fb);
            }

            let start = Instant::now();
            match display.refresh(&fb, mode) {
                Ok(()) => {
                    let ms = start.elapsed().as_millis();
                    total_ms += ms;
                    info!("{} update {} in {} ms", name, count, ms);
                }
                Err(_) => error!("{} update failed", name),
            }
            delay.delay_millis(2000);
        }
        info!(
            "{} average {} ms",
            name,
            total_ms / u64::from(UPDATES_PER_MODE)
        );
    }
    info!("all modes done");
    loop {
        delay.delay_millis(1000);
    }
}
