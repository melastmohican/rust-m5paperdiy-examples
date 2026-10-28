//! # epdpi_pmic_dump: PMIC register dump (M5Stack PaperDIY)
//!
//! Library diagnostic. Not part of the normal bring-up order.
//!
//! Read-only. Prints the M5PM1 registers that control rails and GPIOs without writing
//! anything. Run it in a state where the panel works (for example right after an Arduino
//! M5Unified sketch), then again after a full power cycle, and diff the two logs. Registers
//! that differ show what a working setup configures and `pmic::epd_power_on` does not.
//!
//! Register `0x06` PWR_CFG bits: 0 charge, 1 DC-DC 3V3, 2 LDO, 3 5V boost, 4 LED. The IRQ
//! status registers (`0x40..0x42`) are skipped because reading them may clear them.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_pmic_dump
//! ```
//!
//! With no battery attached, unplugging USB removes all power from the PMIC, which gives a
//! clean cold start for the second dump.
//!
//! ## Result on the author's board
//!
//! A cold PMIC already reads `PWR_CFG = 0x17`. The only differences between a cold and a
//! working state were GPIO2 (EPD_PWR, which `epd_power_on` sets) and a pull-up on GPIO3
//! (charge status, unrelated to the panel). The PMIC was therefore not the cause of the
//! early failures.
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
