//! # Clear (M5Stack PaperDIY)
//!
//! Step 3 of bring-up, the first time the driver moves the panel. Powers the PMIC rail, then
//! runs `EpdDisplay::clear`: 8 black passes, 8 white passes and a discharge pass.
//!
//! ## What to watch
//!
//! - The screen flashes fully black, then fully white. Any stripes or half-height
//!   regions point at the data bus order or CKV/SPV timing.
//! - Total time is logged. Expect a few seconds with the bit-banged bus.
//!
//! Power-cycle between runs after interrupting one. An interrupted refresh can leave the
//! gate drivers in an odd state.
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

    let bus = rust_m5paperdiy_examples::display_pins!(p).into_bus();
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
