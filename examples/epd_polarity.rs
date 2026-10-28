//! # Polarity Test (M5Stack PaperDIY)
//!
//! Debug step between `epd_pins` and `epd_clear`. Talks to the bus directly, no waveform
//! engine, and drives the whole panel one direction at a time:
//!
//! 1. **Darken** (`0x55`, data pins D0 D2 D4 D6) for 30 passes, then holds 10 seconds.
//! 2. **Lighten** (`0xAA`, data pins D1 D3 D5 D7) for 30 passes, then holds 10 seconds.
//! 3. Repeats.
//!
//! ## What to watch
//!
//! - After stage 1 the panel should be clearly darker or solid black.
//! - After stage 2 it should be clearly lighter or solid white.
//! - If only one stage changes the panel, the other direction's data pins are not
//!   reaching the source drivers. That points at the pin map, not the waveform.
//! - If neither changes it, the fault is in power or the gate/latch lines.
#![no_std]
#![no_main]

use alloc::vec;
use defmt::{error, info};
use esp_hal::delay::Delay;
use esp_println as _;
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

const PASSES: usize = 30;

fn drive<B: EpdParallelBus>(bus: &mut B, line: &[u8]) -> Result<(), B::Error> {
    bus.set_power(true)?;
    for _ in 0..PASSES {
        bus.start_frame()?;
        for _ in 0..Ed047tc1::HEIGHT {
            bus.write_scanline(line)?;
            bus.next_row()?;
        }
        bus.end_frame()?;
    }
    // Neutral pass so the drivers discharge.
    let zero = vec![0u8; line.len()];
    bus.start_frame()?;
    for _ in 0..Ed047tc1::HEIGHT {
        bus.write_scanline(&zero)?;
        bus.next_row()?;
    }
    bus.end_frame()?;
    bus.set_power(false)
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let mut bus = rust_m5paperdiy_examples::display_pins!(p).into_bus();
    loop {
        for (name, byte) in [("DARKEN 0x55", 0x55u8), ("LIGHTEN 0xAA", 0xAAu8)] {
            info!("{} for {} passes", name, PASSES);
            let line = vec![byte; Ed047tc1::SCANLINE_BYTES];
            if drive(&mut bus, &line).is_err() {
                error!("bus error");
            }
            info!("{} done, look at the panel now (10 s)", name);
            delay.delay_millis(10_000);
        }
    }
}
