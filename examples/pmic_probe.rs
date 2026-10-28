//! # PMIC Probe (M5Stack PaperDIY)
//!
//! Step 1 of bring-up, no display driver involved. Reads the M5PM1 device ID over I2C and
//! switches the panel rail on through PMIC GPIO2.
//!
//! ## Expected log
//!
//! ```text
//! M5PM1 id 0x2050 (expected 0x2050)
//! EPD_PWR high via PMIC GPIO2
//! ```
//!
//! If the ID read fails or differs, stop here. Check SDA = GPIO41, SCL = GPIO42 and that the
//! board really is a PaperDIY (the PMIC address is 0x6E).
#![no_std]
#![no_main]

use defmt::{error, info};
use esp_hal::delay::Delay;
use esp_println as _;
use rust_m5paperdiy_examples::pmic;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();

    match pmic::read_id(&mut i2c) {
        Ok(id) => info!("M5PM1 id {:#06x} (expected {:#06x})", id, pmic::DEVICE_ID),
        Err(_) => error!("M5PM1 did not answer at {:#04x}", pmic::ADDR),
    }
    match pmic::epd_power_on(&mut i2c, &mut delay) {
        Ok(()) => info!("EPD_PWR high via PMIC GPIO2"),
        Err(_) => error!("PMIC GPIO2 write failed"),
    }
    loop {
        delay.delay_millis(1000);
    }
}
