//! # poc_battery: can Rust read the M5PM1 battery voltage? (M5Stack PaperDIY)
//!
//! Proof of concept for the `battery` port and the `factory_test` checklist. Reads the M5PM1
//! ADC registers once a second for ten seconds: VBAT (`0x22`, `0x23`), VIN (`0x24`, `0x25`),
//! 5VINOUT (`0x26`, `0x27`) and VREF (`0x20`, `0x21`), each a little endian value in mV, the
//! way the Arduino `M5PM1` library does. No display.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example poc_battery
//! ```
//!
//! ## Expected
//!
//! - `device id 2050` first, as in `epdpi_pmic_probe`.
//! - Ten lines `vbat N mV, vin N mV, 5v N mV, vref N mV`.
//! - With a battery fitted VBAT is a plausible Li-ion value, 3000 to 4400 mV, which is the
//!   Arduino factory test's pass range. With USB only, VIN is about 5000 mV and VBAT reads
//!   whatever the ADC sees with nothing connected.
#![no_std]
#![no_main]

use defmt::{error, info};
use esp_hal::delay::Delay;
use esp_println as _;
use rust_m5paperdiy_examples::pmic;

esp_bootloader_esp_idf::esp_app_desc!();

/// Reads a little endian 16 bit register pair.
fn read_mv<I: embedded_hal::i2c::I2c>(i2c: &mut I, reg: u8) -> Option<u16> {
    let mut raw = [0u8; 2];
    i2c.write_read(pmic::ADDR, &[reg], &mut raw).ok()?;
    Some(u16::from_le_bytes(raw))
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let delay = Delay::new();

    match pmic::read_id(&mut i2c) {
        Ok(id) => info!("device id {:04x}", id),
        Err(_) => {
            error!("no answer from the M5PM1");
            rust_m5paperdiy_examples::halt();
        }
    }
    for _ in 0..10 {
        let vbat = read_mv(&mut i2c, 0x22);
        let vin = read_mv(&mut i2c, 0x24);
        let v5 = read_mv(&mut i2c, 0x26);
        let vref = read_mv(&mut i2c, 0x20);
        info!(
            "vbat {} mV, vin {} mV, 5v {} mV, vref {} mV",
            vbat, vin, v5, vref
        );
        delay.delay_millis(1000);
    }
    info!("done");
    loop {
        delay.delay_millis(1000);
    }
}
