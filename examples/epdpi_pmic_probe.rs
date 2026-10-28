//! # epdpi_pmic_probe: PMIC probe (M5Stack PaperDIY)
//!
//! Library diagnostic, step 1 of 6. No display driver involved.
//!
//! The PaperDIY powers the e-paper rail through an M5PM1 power-management chip on I2C
//! (SDA GPIO41, SCL GPIO42, address 0x6E). This example reads the chip's device ID, then
//! runs `pmic::epd_power_on`, which makes PMIC GPIO2 a push-pull output and drives it high
//! (EPD_PWR). The sequence is the one M5GFX uses at board detection.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_pmic_probe
//! ```
//!
//! ## Expected log
//!
//! ```text
//! M5PM1 id 0x2050 (expected 0x2050)
//! EPD_PWR high via PMIC GPIO2
//! ```
//!
//! ## If it fails
//!
//! - **No ID or a different one:** check that the board is a PaperDIY. The PMIC address is
//!   0x6E. 0x6F is the M5IOE1 expander on other M5 boards, and an older design note that
//!   listed 0x6F was wrong.
//! - **ID right, write fails:** the I2C bus is shared with nothing else on this board, so
//!   suspect the pin assignment.
//!
//! The PMIC keeps its GPIO state across an ESP32 reset, so EPD_PWR stays high until the
//! board loses power.
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
