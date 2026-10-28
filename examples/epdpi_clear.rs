//! # epdpi_clear: first panel movement (M5Stack PaperDIY)
//!
//! Library diagnostic, step 3 of 6. The first time `epdpi` moves the panel.
//!
//! Switches the PMIC rail on, then runs `EpdDisplay::clear`: 8 darken passes, 8 lighten
//! passes and one discharge pass. The panel should end up clean white. The example uses the
//! register-level `FastBus`. The portable `GenericGpioBus` also clears the panel but takes
//! about 15 times longer per row (see `epdpi_gray_ramp` for why that matters).
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_clear
//! ```
//!
//! ## Expected
//!
//! - The screen goes to black, then to white. It ends clean with no old content.
//! - The log prints `clear done in N ms`. Expect well under one second with `FastBus`.
//!
//! ## If it fails
//!
//! - **Noise or dark gray after the run:** the line sequence on the bus is wrong. This is
//!   what an early `GenericGpioBus` produced (short LE pulse, no wait after power-on). The
//!   current sequence follows M5GFX's `Bus_EPD`. Run `epdpi_timing` to compare variants.
//! - **No change at all:** run `epdpi_polarity` to separate "no drive" from "one direction
//!   missing". Also check that the FPC cable is seated.
//! - Power-cycle after an interrupted refresh before judging the next run.
#![no_std]
#![no_main]

use alloc::vec;
use defmt::{error, info};
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

    let bus = rust_m5paperdiy_examples::display_pins!(p).into_fast_bus();
    let mut prev = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let Ok(mut display) = EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line)
    else {
        error!("buffer allocation too small");
        rust_m5paperdiy_examples::halt();
    };

    let start = Instant::now();
    match display.clear() {
        Ok(()) => info!("clear done in {} ms", start.elapsed().as_millis()),
        Err(_) => error!("clear failed"),
    }
    loop {
        delay.delay_millis(1000);
    }
}
