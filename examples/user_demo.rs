//! # user_demo: port of the Arduino `PaperDIYUserDemo` sketch (M5Stack PaperDIY)
//!
//! The M5Stack PaperDIY demo: a 540 by 960 poster with live widgets on top, refreshed in small
//! windows. It is ported in stages, see the `user_demo` item in the `epdpi` backlog.
//!
//! **Stages 1 and 2 (this file):** the background image, the BtnA and BtnB border feedback and
//! the battery voltage. The
//! async twin is `user_demo` in `rust-m5paperdiy-embassy-examples`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example user_demo
//! ```
//!
//! ## Expected
//!
//! - The poster draws with `RefreshMode::M5Quality` (about 1 s), portrait, connector at the bottom.
//!   The log prints `background decoded in N ms, drawn in N ms`.
//! - Holding BtnA (GPIO4) draws a black border around the `btnA G4` label, releasing it draws
//!   the border white again. BtnB (GPIO3) does the same around `btnB G3`. Each change refreshes
//!   only that area with `RefreshMode::M5Fastest` and logs `A down, B up: border refresh N ms`.
//! - **Stage 2:** the battery box on the poster shows the voltage, `4.03V`, from the first
//!   frame and updates every 10 s (10 samples averaged) with `RefreshMode::M5Text`, only when the
//!   text changes and never while a button is down. The log prints `battery 4.03V: refresh N ms`.
//! - Not in this stage: Wi-Fi list, card test and the power-off dialog.
//! - The artwork is M5Stack's, copied from the Arduino sketch (`assets/img_bg.png`).
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Pull};
use esp_hal::time::Instant;
use esp_println as _;
use rust_m5paperdiy_examples::{epdpi, pmic};

use epdpi::prelude::*;

esp_bootloader_esp_idf::esp_app_desc!();

/// The poster, an 8 bit gray PNG, 540 by 960.
const BACKGROUND: &[u8] = include_bytes!("../assets/img_bg.png");

/// Drawing helpers. No hardware in here, and this module is the same text in the blocking and
/// the async crate. The crate root must `use <crate>::epdpi;` for the paths below.
mod ui {
    use core::fmt::Write;

    use embedded_graphics::{pixelcolor::Gray4, prelude::*};
    use u8g2_fonts::{
        FontRenderer, fonts,
        types::{FontColor, HorizontalAlignment, VerticalPosition},
    };

    use crate::epdpi::prelude::*;

    pub type Fb<'a> = EpdFrameBuffer<'a, Ed047tc1>;

    /// M5GFX's ordered dither matrix (`Panel_EPD.inl`), indexed by `(y & 3) * 4 + (x & 3)`.
    const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

    /// 8 bit gray to the panel's 16 levels the way M5GFX's `writeImage` does it.
    pub fn quantize(value: u8, x: u32, y: u32) -> u8 {
        let bayer = i32::from(BAYER[((y & 3) * 4 + (x & 3)) as usize]);
        ((i32::from(value) + bayer - 8) >> 4).clamp(0, 15) as u8
    }

    /// Draws a `width` by `height` 8 bit gray image at the top left.
    pub fn draw_gray8(fb: &mut Fb<'_>, pixels: &[u8], width: u32, height: u32) {
        for y in 0..height {
            let row = &pixels[(y * width) as usize..((y + 1) * width) as usize];
            for (x, &value) in row.iter().enumerate() {
                fb.set_logical_pixel(x as u32, y, quantize(value, x as u32, y));
            }
        }
    }

    /// Left edge, top of the A and B labels, right edge and height of the border.
    pub const BORDER_X: i32 = 9;
    pub const BORDER_A_Y: i32 = 620;
    pub const BORDER_B_Y: i32 = 731;
    pub const BORDER_RIGHT: i32 = 119;
    pub const BORDER_H: i32 = 67;
    const THICKNESS: i32 = 2;
    const RADIUS: i32 = 17;

    fn fill(fb: &mut Fb<'_>, x: i32, y: i32, w: i32, h: i32, gray: u8) {
        for yy in y..y + h {
            for xx in x..x + w {
                if xx >= 0 && yy >= 0 {
                    fb.set_logical_pixel(xx as u32, yy as u32, gray);
                }
            }
        }
    }

    /// A quarter ring of `RADIUS` and thickness `THICKNESS` on the right side: `upper` is the
    /// top right quarter around `(cx, cy)`, otherwise the bottom right one.
    fn right_corner(fb: &mut Fb<'_>, cx: i32, cy: i32, upper: bool, gray: u8) {
        let inner = RADIUS - THICKNESS + 1;
        for dy in -RADIUS..=RADIUS {
            if (dy <= 0) != upper && dy != 0 {
                continue;
            }
            for dx in 0..=RADIUS {
                let rr = dx * dx + dy * dy;
                if rr <= RADIUS * RADIUS && rr >= inner * inner {
                    fb.set_logical_pixel((cx + dx) as u32, (cy + dy) as u32, gray);
                }
            }
        }
    }

    /// The border around a button label whose top is `y`: black while pressed, white after.
    /// Left corners are square, right corners are rounded, as in the sketch.
    pub fn draw_border(fb: &mut Fb<'_>, y: i32, pressed: bool) {
        let gray = if pressed { 0 } else { 15 };
        let right_center = BORDER_RIGHT - RADIUS;
        let bottom = y + BORDER_H - 1;
        right_corner(fb, right_center, y + RADIUS, true, gray);
        right_corner(fb, right_center, bottom - RADIUS, false, gray);
        let bar = right_center - BORDER_X + 1;
        fill(fb, BORDER_X, y, bar, THICKNESS, gray);
        fill(fb, BORDER_X, bottom - THICKNESS + 1, bar, THICKNESS, gray);
        fill(fb, BORDER_X, y, THICKNESS, BORDER_H, gray);
        fill(
            fb,
            BORDER_RIGHT - THICKNESS + 1,
            y + RADIUS,
            THICKNESS,
            BORDER_H - RADIUS * 2,
            gray,
        );
    }

    /// A short text in a fixed buffer.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct Text {
        buf: [u8; 16],
        len: usize,
    }

    impl Text {
        pub const fn new() -> Self {
            Self {
                buf: [0; 16],
                len: 0,
            }
        }

        pub fn as_str(&self) -> &str {
            core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
        }
    }

    impl Write for Text {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let end = self.len + s.len();
            let dst = self.buf.get_mut(self.len..end).ok_or(core::fmt::Error)?;
            dst.copy_from_slice(s.as_bytes());
            self.len = end;
            Ok(())
        }
    }

    /// The battery box on the poster: top left, width and height.
    pub const BATTERY_X: i32 = 159;
    pub const BATTERY_Y: i32 = 888;
    pub const BATTERY_W: i32 = 241 - BATTERY_X;
    pub const BATTERY_H: i32 = 925 - BATTERY_Y;

    const SAMPLE_COUNT: u32 = 10;
    const SAMPLE_EVERY_MS: u64 = 50;
    const REFRESH_EVERY_MS: u64 = 10_000;

    /// The battery voltage widget, the `AppBattery` of the sketch: every 10 s it takes 10 samples
    /// 50 ms apart, averages them and shows `4.03V`. It only asks for a redraw when the text
    /// changed, and holds a finished reading back while a button is down.
    pub struct Battery {
        started: bool,
        sampling: bool,
        force: bool,
        sum: i32,
        count: u32,
        valid: u32,
        last_sample_ms: u64,
        last_update_ms: u64,
        pending: Option<(i16, bool)>,
        last_text: Text,
    }

    impl Battery {
        pub const fn new() -> Self {
            Self {
                started: false,
                sampling: false,
                force: false,
                sum: 0,
                count: 0,
                valid: 0,
                last_sample_ms: 0,
                last_update_ms: 0,
                pending: None,
                last_text: Text::new(),
            }
        }

        fn start_sampling<F: FnMut() -> Option<u16>>(&mut self, now_ms: u64, force: bool, read: F) {
            self.sum = 0;
            self.count = 0;
            self.valid = 0;
            self.sampling = true;
            self.force = force;
            self.collect(now_ms, read);
        }

        fn collect<F: FnMut() -> Option<u16>>(&mut self, now_ms: u64, mut read: F) {
            self.last_sample_ms = now_ms;
            if let Some(mv) = read().filter(|&mv| mv > 0) {
                self.sum += i32::from(mv);
                self.valid += 1;
            }
            self.count += 1;
            if self.count < SAMPLE_COUNT {
                return;
            }
            let average = if self.valid > 0 {
                ((self.sum + self.valid as i32 / 2) / self.valid as i32) as i16
            } else {
                -1
            };
            self.sampling = false;
            self.last_update_ms = self.last_sample_ms;
            let force = self.force || self.pending.is_some_and(|(_, force)| force);
            self.pending = Some((average, force));
        }

        /// Call every few milliseconds. `read` gives the battery voltage in mV, `interaction` is
        /// true while a button is down. Returns the text to draw when the box needs redrawing.
        pub fn poll<F: FnMut() -> Option<u16>>(
            &mut self,
            now_ms: u64,
            interaction: bool,
            read: F,
        ) -> Option<Text> {
            if !self.started {
                self.started = true;
                self.start_sampling(now_ms, true, read);
                return None;
            }
            if let Some((mv, force)) = self.pending {
                if interaction {
                    return None;
                }
                self.pending = None;
                let mut text = Text::new();
                if mv > 0 {
                    let _ = write!(text, "{:.2}V", f32::from(mv) / 1000.0);
                } else {
                    let _ = write!(text, "--.--V");
                }
                if !force && text == self.last_text {
                    return None;
                }
                self.last_text = text;
                return Some(text);
            }
            if self.sampling {
                if now_ms - self.last_sample_ms >= SAMPLE_EVERY_MS {
                    self.collect(now_ms, read);
                }
            } else if now_ms - self.last_update_ms >= REFRESH_EVERY_MS {
                self.start_sampling(now_ms, false, read);
            }
            None
        }
    }

    impl Default for Battery {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Clears the battery box and writes `text` in its middle.
    pub fn draw_battery(fb: &mut Fb<'_>, text: &str) {
        fill(fb, BATTERY_X, BATTERY_Y, BATTERY_W, BATTERY_H, 15);
        let font = FontRenderer::new::<fonts::u8g2_font_fub17_tf>();
        let _ = font.render_aligned(
            format_args!("{text}"),
            Point::new(BATTERY_X + BATTERY_W / 2, BATTERY_Y + BATTERY_H / 2),
            VerticalPosition::Center,
            HorizontalAlignment::Center,
            FontColor::Transparent(Gray4::new(0)),
            fb,
        );
    }
}

/// Battery voltage in mV from the M5PM1 (registers 0x22 and 0x23, little endian).
fn battery_mv<I: I2c>(i2c: &mut I) -> Option<u16> {
    let mut raw = [0u8; 2];
    i2c.write_read(pmic::ADDR, &[0x22], &mut raw).ok()?;
    Some(u16::from_le_bytes(raw))
}

fn now_ms() -> u64 {
    Instant::now().duration_since_epoch().as_millis()
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let config = InputConfig::default().with_pull(Pull::Up);
    let btn_a = Input::new(p.GPIO4, config);
    let btn_b = Input::new(p.GPIO3, config);

    let bus = rust_m5paperdiy_examples::lcd_bus!(p, 16);
    let mut prev = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let (Ok(mut display), Ok(mut fb)) = (
        EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line),
        EpdFrameBuffer::<Ed047tc1>::new(&mut fb_mem),
    ) else {
        error!("buffer allocation too small");
        rust_m5paperdiy_examples::halt();
    };
    fb.set_rotation(Rotation::Deg270);
    if display.clear().is_err() {
        error!("initial clear failed");
    }

    // Decode the poster into a scratch buffer in PSRAM, draw it, drop the buffer.
    let start = now_ms();
    let Ok(header) = minipng::decode_png_header(BACKGROUND) else {
        error!("background is not a PNG");
        rust_m5paperdiy_examples::halt();
    };
    let mut scratch = vec![0u8; header.required_bytes()];
    let Ok(image) = minipng::decode_png(BACKGROUND, &mut scratch) else {
        error!("background decode failed");
        rust_m5paperdiy_examples::halt();
    };
    let decoded = now_ms() - start;
    ui::draw_gray8(&mut fb, image.pixels(), image.width(), image.height());
    drop(scratch);
    let drawn = now_ms() - start - decoded;
    info!(
        "background decoded in {} ms, drawn in {} ms",
        decoded, drawn
    );

    // The first voltage reading, 10 samples half a second apart, goes onto the poster before it
    // is shown, like the sketch.
    let mut battery = ui::Battery::new();
    loop {
        if let Some(text) = battery.poll(now_ms(), false, || battery_mv(&mut i2c)) {
            ui::draw_battery(&mut fb, text.as_str());
            info!("battery {}", text.as_str());
            break;
        }
        delay.delay_ms(5);
    }

    if display.refresh(&fb, RefreshMode::M5Quality).is_err() {
        error!("background refresh failed");
    }
    // The first refresh showed everything, only later changes are dirty.
    fb.clear_dirty();
    info!("ready");

    let (mut a_down, mut b_down) = (false, false);
    loop {
        let (a, b) = (btn_a.is_low(), btn_b.is_low());
        if a != a_down || b != b_down {
            let start = now_ms();
            if a != a_down {
                ui::draw_border(&mut fb, ui::BORDER_A_Y, a);
            }
            if b != b_down {
                ui::draw_border(&mut fb, ui::BORDER_B_Y, b);
            }
            if display
                .refresh_dirty(&mut fb, RefreshMode::M5Fastest)
                .is_err()
            {
                error!("border refresh failed");
            }
            info!(
                "A {}, B {}: border refresh {} ms",
                if a { "down" } else { "up" },
                if b { "down" } else { "up" },
                now_ms() - start
            );
            (a_down, b_down) = (a, b);
        } else if let Some(text) = battery.poll(now_ms(), a || b, || battery_mv(&mut i2c)) {
            // A new reading, shown in its box only.
            let start = now_ms();
            ui::draw_battery(&mut fb, text.as_str());
            if display.refresh_dirty(&mut fb, RefreshMode::M5Text).is_err() {
                error!("battery refresh failed");
            }
            info!("battery {}: refresh {} ms", text.as_str(), now_ms() - start);
        }
        delay.delay_ms(5);
    }
}
