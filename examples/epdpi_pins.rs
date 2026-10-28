//! # epdpi_pins: display pin walk (M5Stack PaperDIY)
//!
//! Library diagnostic, step 2 of 6. No display driver involved.
//!
//! Drives each of the 15 display GPIOs high for 2 seconds, one at a time, and logs which one
//! is active. With a meter or scope on the matching FPC pin or test point this confirms the
//! pin map. Without one it still proves the firmware runs and every pin can be configured.
//! The panel does not change.
//!
//! The PMIC rail is switched on first so the level shifters are powered. PWR (GPIO46) is
//! raised only in the last step and no scanline is clocked, so nothing is driven into the
//! glass.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_pins
//! ```
//!
//! ## Expected log
//!
//! One `<signal> <gpio> high` line every 2 seconds, in the order D0..D7, CKV, SPV, CL, LE,
//! SPH, OE, PWR, then repeating. Stop it with Ctrl+C.
//!
//! ## Pin map
//!
//! D0..D7 = GPIO 6, 14, 7, 12, 9, 11, 8, 10. CKV 18, SPV 17, CL 16, LE 15, SPH 13, OE 45,
//! PWR 46. Source: M5GFX `board_M5PaperDIY`.
#![no_std]
#![no_main]

use defmt::info;
use esp_hal::delay::Delay;
use esp_println as _;
use rust_m5paperdiy_examples::pmic;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let pins = rust_m5paperdiy_examples::display_pins!(p);
    let [d0, d1, d2, d3, d4, d5, d6, d7] = pins.data;
    let c = pins.control;
    let mut all = [
        ("D0 GPIO6", d0),
        ("D1 GPIO14", d1),
        ("D2 GPIO7", d2),
        ("D3 GPIO12", d3),
        ("D4 GPIO9", d4),
        ("D5 GPIO11", d5),
        ("D6 GPIO8", d6),
        ("D7 GPIO10", d7),
        ("CKV GPIO18", c.ckv),
        ("SPV GPIO17", c.spv),
        ("CL GPIO16", c.cl),
        ("LE GPIO15", c.le),
        ("SPH GPIO13", c.sph),
        ("OE GPIO45", c.oe),
        ("PWR GPIO46", c.pwr),
    ];

    loop {
        for (name, pin) in all.iter_mut() {
            info!("{} high", *name);
            pin.set_high();
            delay.delay_millis(2000);
            pin.set_low();
        }
    }
}
