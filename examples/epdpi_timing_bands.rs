//! # epdpi_timing_bands: four row timings in one refresh (M5Stack PaperDIY)
//!
//! Library diagnostic. Draws the 16-level ramp with `RefreshMode::M5Quality`, but the bus
//! drives four horizontal bands of rows with different CKV timing, so one look compares them.
//! The row data phase of `FastBus` takes about 62 us, M5GFX's about 15.5 us (see
//! `epdpi_row_time`), and this checks whether the CKV high time is what the waveform depends on.
//!
//! | Band | Position | CKV |
//! | :--- | :--- | :--- |
//! | A | top | low during data, then high 2.5 us |
//! | B | second | low during data, then high 1.5 us |
//! | C | third | low during data, then high 1 us |
//! | D | bottom | low during data, then high 0.5 us |
//!
//! An earlier run showed 62 us and 15 us pulses look the same and 4 us is lighter, so these
//! go shorter to look for the pulse length that gives an even ramp.
//!
//! The ramp has 16 bars from black (left) to white (right), starting right of a white column
//! that carries the band letters.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_timing_bands
//! ```
//!
//! ## Expected
//!
//! The band whose ramp has the most evenly spaced grays has the right timing. A band that
//! shows nothing or garbage means the panel needs CKV high while the data is clocked.
#![no_std]
#![no_main]

use alloc::vec;
use core::convert::Infallible;
use defmt::{error, info};
use embedded_graphics::{
    mono_font::MonoTextStyle,
    pixelcolor::Gray4,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use esp_hal::delay::Delay;
use esp_hal::gpio::Output;
use esp_hal::time::Instant;
use esp_println as _;
use profont::PROFONT_24_POINT;
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

const OUT_W1TS: *mut u32 = 0x6000_4008 as *mut u32;
const OUT_W1TC: *mut u32 = 0x6000_400C as *mut u32;
const DATA_GPIO: [u32; 8] = [6, 14, 7, 12, 9, 11, 8, 10];
const CL_GPIO: u32 = 16;
const BAND_ROWS: usize = 135;

/// `FastBus` with the CKV timing chosen per band of rows.
struct BandBus {
    _data: [Output<'static>; 8],
    c: ControlPins<Output<'static>>,
    delay: Delay,
    table: [u32; 256],
    all_data: u32,
    row: usize,
}

impl BandBus {
    fn new(pins: rust_m5paperdiy_examples::PaperDiyPins) -> Self {
        let mut table = [0u32; 256];
        for (value, slot) in table.iter_mut().enumerate() {
            for (bit, gpio) in DATA_GPIO.iter().enumerate() {
                if value & (1 << bit) != 0 {
                    *slot |= 1 << gpio;
                }
            }
        }
        Self {
            _data: pins.data,
            c: pins.control,
            delay: Delay::new(),
            table,
            all_data: DATA_GPIO.iter().fold(0, |m, g| m | (1 << g)),
            row: 0,
        }
    }
}

impl EpdParallelBus for BandBus {
    type Error = Infallible;

    fn set_power(&mut self, on: bool) -> Result<(), Self::Error> {
        if on {
            self.c.oe.set_high();
            self.delay.delay_micros(100);
            self.c.pwr.set_high();
            self.delay.delay_micros(100);
            self.c.spv.set_high();
            self.c.sph.set_high();
            self.delay.delay_millis(1);
        } else {
            self.delay.delay_millis(1);
            self.c.pwr.set_low();
            self.delay.delay_micros(10);
            self.c.oe.set_low();
            self.delay.delay_micros(100);
            self.c.spv.set_low();
        }
        Ok(())
    }

    fn start_frame(&mut self) -> Result<(), Self::Error> {
        self.row = 0;
        self.c.spv.set_low();
        self.delay.delay_micros(1);
        self.c.ckv.set_low();
        self.delay.delay_micros(3);
        self.c.ckv.set_high();
        self.delay.delay_micros(1);
        self.c.spv.set_high();
        for _ in 0..3 {
            self.delay.delay_micros(3);
            self.c.ckv.set_low();
            self.delay.delay_micros(3);
            self.c.ckv.set_high();
        }
        Ok(())
    }

    fn write_scanline(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let band = (self.row / BAND_ROWS).min(3);
        self.c.le.set_low();
        self.c.sph.set_low();
        let cl = 1u32 << CL_GPIO;
        for &byte in data {
            let ones = self.table[usize::from(byte)];
            // SAFETY: fixed ESP32-S3 GPIO set and clear registers, touching only D0..D7 and
            // CL, which this struct owns.
            unsafe {
                OUT_W1TC.write_volatile(self.all_data & !ones);
                OUT_W1TS.write_volatile(ones);
                OUT_W1TS.write_volatile(cl);
                OUT_W1TC.write_volatile(cl);
            }
        }
        self.c.sph.set_high();
        let high_ns = match band {
            0 => 2500,
            1 => 1500,
            2 => 1000,
            _ => 500,
        };
        self.c.ckv.set_high();
        self.delay.delay_nanos(high_ns);
        Ok(())
    }

    fn next_row(&mut self) -> Result<(), Self::Error> {
        self.c.ckv.set_low();
        self.c.le.set_high();
        self.row += 1;
        Ok(())
    }

    fn end_frame(&mut self) -> Result<(), Self::Error> {
        self.c.le.set_low();
        self.c.ckv.set_high();
        Ok(())
    }
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let bus = BandBus::new(rust_m5paperdiy_examples::display_pins!(p));
    // As the M5GFX sketch does: every pixel starts at mid gray, no hard clear.
    let mut prev = vec![0x88u8; Ed047tc1::FB_BYTES];
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let (Ok(mut display), Ok(mut fb)) = (
        EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line),
        EpdFrameBuffer::<Ed047tc1>::new(&mut fb_mem),
    ) else {
        error!("buffer allocation too small");
        rust_m5paperdiy_examples::halt();
    };

    // White background through the same waveform first.
    let mut white_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    if let Ok(white) = EpdFrameBuffer::<Ed047tc1>::new(&mut white_mem)
        && display.refresh(&white, RefreshMode::M5Quality).is_err()
    {
        error!("white refresh failed");
    }

    fb.fill(0xF);
    for level in 0..16u8 {
        let _ = Rectangle::new(
            Point::new(96 + i32::from(level) * 54, 0),
            Size::new(54, 540),
        )
        .into_styled(PrimitiveStyle::with_fill(Gray4::new(level)))
        .draw(&mut fb);
    }
    let style = MonoTextStyle::new(&PROFONT_24_POINT, Gray4::BLACK);
    for (i, letter) in ["A", "B", "C", "D"].into_iter().enumerate() {
        let y = (i * BAND_ROWS + BAND_ROWS / 2 + 10) as i32;
        let _ = Text::new(letter, Point::new(36, y), style).draw(&mut fb);
    }

    let start = Instant::now();
    match display.refresh(&fb, RefreshMode::M5Quality) {
        Ok(()) => info!("refresh done in {} ms", start.elapsed().as_millis()),
        Err(_) => error!("refresh failed"),
    }
    loop {
        delay.delay_millis(1000);
    }
}
