//! # Bus Timing Variants (M5Stack PaperDIY)
//!
//! Debug step. Replays the M5GFX line sequence (`Bus_EPD.inl`) on raw pins and varies the
//! source-clock pulse width, so a run shows which timing the panel responds to.
//!
//! M5GFX differs from `GenericGpioBus` in three ways, all reproduced here:
//! - waits 1 ms after the power rail comes up,
//! - drops LE *before* a line's data and keeps it high between lines,
//! - holds CL high for a full 16 MHz DMA clock period, not a GPIO write apart.
//!
//! For each variant it darkens the whole panel (30 passes), holds 8 s, lightens it
//! (30 passes), holds 8 s. Watch the panel and note which variant, if any, changes it.
#![no_std]
#![no_main]

use defmt::info;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output};
use esp_println as _;
use rust_m5paperdiy_examples::{PaperDiyPins, pmic};

esp_bootloader_esp_idf::esp_app_desc!();

const PASSES: usize = 30;
const ROWS: usize = 540;
const BYTES_PER_ROW: usize = 248;

struct Raw {
    d: [Output<'static>; 8],
    ckv: Output<'static>,
    spv: Output<'static>,
    sph: Output<'static>,
    oe: Output<'static>,
    le: Output<'static>,
    cl: Output<'static>,
    pwr: Output<'static>,
    delay: Delay,
}

impl Raw {
    fn new(p: PaperDiyPins) -> Self {
        Self {
            d: p.data,
            ckv: p.control.ckv,
            spv: p.control.spv,
            sph: p.control.sph,
            oe: p.control.oe,
            le: p.control.le,
            cl: p.control.cl,
            pwr: p.control.pwr,
            delay: Delay::new(),
        }
    }

    fn power(&mut self, on: bool) {
        if on {
            self.oe.set_high();
            self.delay.delay_micros(100);
            self.pwr.set_high();
            self.delay.delay_micros(100);
            self.spv.set_high();
            self.delay.delay_millis(1);
        } else {
            self.delay.delay_millis(1);
            self.pwr.set_low();
            self.delay.delay_micros(10);
            self.oe.set_low();
            self.delay.delay_micros(100);
            self.spv.set_low();
        }
    }

    fn begin_frame(&mut self) {
        self.spv.set_low();
        self.delay.delay_micros(1);
        self.ckv.set_low();
        self.delay.delay_micros(3);
        self.ckv.set_high();
        self.delay.delay_micros(1);
        self.spv.set_high();
        for _ in 0..3 {
            self.delay.delay_micros(3);
            self.ckv.set_low();
            self.delay.delay_micros(3);
            self.ckv.set_high();
        }
    }

    /// One scanline of a uniform byte, M5GFX order.
    fn line(&mut self, byte: u8, cl_hold_ns: u32) {
        self.le.set_low();
        self.ckv.set_high();
        self.sph.set_low();
        for (bit, pin) in self.d.iter_mut().enumerate() {
            pin.set_level(Level::from(byte & (1 << bit) != 0));
        }
        for _ in 0..BYTES_PER_ROW {
            self.cl.set_high();
            if cl_hold_ns > 0 {
                self.delay.delay_nanos(cl_hold_ns);
            }
            self.cl.set_low();
            if cl_hold_ns > 0 {
                self.delay.delay_nanos(cl_hold_ns);
            }
        }
        self.sph.set_high();
        self.ckv.set_low();
        self.le.set_high();
    }

    fn end_frame(&mut self) {
        self.le.set_low();
        self.ckv.set_high();
    }

    fn drive(&mut self, byte: u8, cl_hold_ns: u32) {
        self.power(true);
        for _ in 0..PASSES {
            self.begin_frame();
            for _ in 0..ROWS {
                self.line(byte, cl_hold_ns);
            }
            self.end_frame();
        }
        // Neutral pass to discharge.
        self.begin_frame();
        for _ in 0..ROWS {
            self.line(0, cl_hold_ns);
        }
        self.end_frame();
        self.power(false);
    }
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let mut raw = Raw::new(rust_m5paperdiy_examples::display_pins!(p));
    let variants: [(&str, u32); 3] = [
        ("V1 CL as fast as GPIO writes", 0),
        ("V2 CL 300 ns high / 300 ns low", 300),
        ("V3 CL 2 us high / 2 us low", 2000),
    ];
    loop {
        for (name, hold) in variants {
            info!("{}: DARKEN", name);
            raw.drive(0x55, hold);
            info!("{}: darken done, look now (8 s)", name);
            delay.delay_millis(8000);
            info!("{}: LIGHTEN", name);
            raw.drive(0xAA, hold);
            info!("{}: lighten done, look now (8 s)", name);
            delay.delay_millis(8000);
        }
    }
}
