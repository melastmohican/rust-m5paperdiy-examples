//! # PMIC Register Dump (M5Stack PaperDIY)
//!
//! Read-only. Prints the M5PM1 registers that control rails and GPIOs, without touching
//! anything. Run it once in a state where the panel works (right after an Arduino
//! M5Unified sketch), then again after a full power cycle, and diff the two logs. The
//! registers that differ are the ones `pmic::epd_power_on` is missing.
//!
//! `0x06` PWR_CFG bits: 0 charge, 1 DC-DC 3V3, 2 LDO, 3 5V boost, 4 LED.
//! IRQ status registers (`0x40..0x42`) are skipped because a read may clear them.
#![no_std]
#![no_main]

use defmt::{info, warn};
use esp_hal::delay::Delay;
use esp_println as _;
use rust_m5paperdiy_examples::pmic;

esp_bootloader_esp_idf::esp_app_desc!();

const REGS: &[(u8, &str)] = &[
    (0x00, "DEVICE_ID_L"),
    (0x01, "DEVICE_ID_H"),
    (0x04, "PWR_SRC"),
    (0x05, "WAKE_SRC"),
    (0x06, "PWR_CFG"),
    (0x09, "I2C_CFG"),
    (0x0A, "WDT_CNT"),
    (0x10, "GPIO_MODE"),
    (0x11, "GPIO_OUT"),
    (0x12, "GPIO_IN"),
    (0x13, "GPIO_DRV"),
    (0x14, "GPIO_PUPD0"),
    (0x15, "GPIO_PUPD1"),
    (0x16, "GPIO_FUNC0"),
    (0x17, "GPIO_FUNC1"),
    (0x22, "VBAT_L"),
    (0x23, "VBAT_H"),
    (0x24, "VIN_L"),
    (0x25, "VIN_H"),
    (0x26, "5VOUT_L"),
    (0x27, "5VOUT_H"),
    (0x43, "IRQ_MASK1"),
    (0x44, "IRQ_MASK2"),
    (0x45, "IRQ_MASK3"),
];

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let delay = Delay::new();

    info!("--- M5PM1 dump (no writes) ---");
    for &(reg, name) in REGS {
        let mut v = [0u8; 1];
        match embedded_hal::i2c::I2c::write_read(&mut i2c, pmic::ADDR, &[reg], &mut v) {
            Ok(()) => info!("{:#04x} {} = {:#04x} ({:#010b})", reg, name, v[0], v[0]),
            Err(_) => warn!("{:#04x} {} read failed", reg, name),
        }
    }
    info!("--- end ---");
    loop {
        delay.delay_millis(1000);
    }
}
