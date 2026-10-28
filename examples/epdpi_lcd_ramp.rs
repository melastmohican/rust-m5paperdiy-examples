//! # epdpi_lcd_ramp: 16-level grayscale over the LCD_CAM DMA bus (M5Stack PaperDIY)
//!
//! Library diagnostic. The `epdpi_gray_ramp_m5` ramp (`RefreshMode::M5Quality`, M5GFX's
//! waveform) on `lcd_bus::LcdBus` instead of GPIO bit-banging. The row data phase is then
//! set by the 16 MHz bus clock, about 15.5 us for 248 bytes, as in M5GFX.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epdpi_lcd_ramp
//! ```
//!
//! ## Expected
//!
//! - The log prints `refresh done in N ms`.
//! - 16 bars from black to white, a white strip below reading `Rust: epdpi LCD_CAM M5Quality`.
//! - An even ramp like the M5GFX `EpdQualityRamp` sketch, without the dark band of the
//!   GPIO bus.
#![no_std]
#![no_main]

use alloc::vec;
use defmt::{error, info};
use embedded_graphics::{
    mono_font::MonoTextStyle,
    pixelcolor::Gray4,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use esp_hal::delay::Delay;
use esp_hal::time::Instant;
use esp_println as _;
use profont::PROFONT_24_POINT;
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

extern crate alloc;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let bus = rust_m5paperdiy_examples::lcd_bus!(p, 16);
    // M5GFX assumes every pixel starts at mid gray (0x88) and has no hard clear.
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

    for level in 0..16u8 {
        let _ = Rectangle::new(Point::new(i32::from(level) * 60, 0), Size::new(60, 480))
            .into_styled(PrimitiveStyle::with_fill(Gray4::new(level)))
            .draw(&mut fb);
    }
    let style = MonoTextStyle::new(&PROFONT_24_POINT, Gray4::BLACK);
    let _ = Text::new("Rust: epdpi LCD_CAM M5Quality", Point::new(40, 520), style).draw(&mut fb);

    // As the M5GFX sketch does: a white background through the same waveform first.
    let mut white_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    if let Ok(white) = EpdFrameBuffer::<Ed047tc1>::new(&mut white_mem)
        && display.refresh(&white, RefreshMode::M5Quality).is_err()
    {
        error!("white refresh failed");
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
