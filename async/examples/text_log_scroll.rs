//! # text_log_scroll: async port of the Arduino `TextLogScroll` sketch (M5Stack PaperDIY)
//!
//! Prints `0000:hello world`, `0001:this`, `0002:is`, ... one line per update in big black
//! letters on white, scrolling up when the screen is full, as fast as the panel allows, with
//! `RefreshMode::M5Fastest` like the sketch's `epd_fastest`. The blocking twin is
//! `text_log_scroll` in `rust-m5paperdiy-examples`. Here `epdpi` runs in async mode on the
//! `esp-rtos` Embassy executor.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example text_log_scroll
//! ```
//!
//! ## Expected
//!
//! - Landscape, like the sketch (it turns a portrait screen to landscape).
//! - The log prints `line N: update in X ms` every 8 lines. Each line is a full screen update.
//! - Lines never stop. After the screen fills, the top line goes and the rest move up.
//! - Differences from the Arduino sketch: the sketch blows its 6x8 font up 6 times, which gives
//!   blocky letters. This draws a bold monospace `u8g2-fonts` font (`inb30`) at its own size, so
//!   the edges are smooth, with a different number of lines on screen (the log prints it). The
//!   sketch's 1-bit canvas is a 4 bit buffer here.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use core::fmt::Write;
use defmt::{error, info};
use embassy_time::Instant;
use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use esp_hal::delay::Delay;
use esp_println as _;
use rust_m5paperdiy_embassy_examples::{epdpi::prelude::*, pmic};
use u8g2_fonts::{
    FontRenderer, fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
};

esp_bootloader_esp_idf::esp_app_desc!();

/// Which build this is, so a photo says it.
const BUILD: &str = "Rust: epdpi async";

/// Draws `BUILD` small in the top right corner, where the log lines do not reach.
fn draw_label(fb: &mut EpdFrameBuffer<'_, Ed047tc1>) {
    let font = FontRenderer::new::<fonts::u8g2_font_helvR12_tf>();
    let (w, _) = fb.logical_size();
    let _ = font.render_aligned(
        format_args!("{BUILD}"),
        Point::new(w as i32 - 12, 14),
        VerticalPosition::Center,
        HorizontalAlignment::Right,
        FontColor::Transparent(Gray4::BLACK),
        fb,
    );
}

/// What the sketch prints, one word per line, in turn.
const WORDS: [&str; 8] = [
    "hello world",
    "this",
    "is",
    "text",
    "log",
    "vertical",
    "scroll",
    "sample",
];
/// Most text rows ever on screen, the size of the line store.
const MAX_ROWS: usize = 16;
/// Longest line: `0000:` plus the longest word, with room to spare.
const LINE_LEN: usize = 24;

/// One line of the log, a tiny fixed-capacity string.
#[derive(Clone, Copy)]
struct Line {
    buf: [u8; LINE_LEN],
    len: usize,
}

impl Line {
    const EMPTY: Self = Self {
        buf: [0; LINE_LEN],
        len: 0,
    };

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let end = self.len + bytes.len();
        let dst = self.buf.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        dst.copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }
}

/// Draws the visible lines on a white page, oldest at the top.
fn draw_log(
    fb: &mut EpdFrameBuffer<'_, Ed047tc1>,
    lines: &[Line],
    font: &FontRenderer,
    pitch: i32,
) {
    fb.fill(0xF);
    for (row, line) in lines.iter().enumerate() {
        let _ = font.render_aligned(
            line.as_str(),
            Point::new(0, row as i32 * pitch),
            VerticalPosition::Top,
            HorizontalAlignment::Left,
            FontColor::Transparent(Gray4::BLACK),
            fb,
        );
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_m5paperdiy_embassy_examples::init!();
    rust_m5paperdiy_embassy_examples::start_rtos!(p);
    let mut i2c = rust_m5paperdiy_embassy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let bus = rust_m5paperdiy_embassy_examples::lcd_bus!(p, 16);
    let mut prev = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut line_buf = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let (Ok(mut display), Ok(mut fb)) = (
        EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line_buf),
        EpdFrameBuffer::<Ed047tc1>::new(&mut fb_mem),
    ) else {
        error!("buffer allocation too small");
        rust_m5paperdiy_embassy_examples::halt();
    };

    fb.set_rotation(Rotation::Deg0);
    if display.clear().await.is_err() {
        error!("initial clear failed");
    }

    // A bold monospace font at its own size, not a small one blown up, so the edges stay smooth.
    let font = FontRenderer::new::<fonts::u8g2_font_inb30_mf>();
    let pitch = font.get_default_line_height() as i32;
    let rows = (fb.logical_size().1 as i32 / pitch).clamp(1, MAX_ROWS as i32) as usize;
    info!("{} rows of {} px", rows, pitch);
    let mut lines = [Line::EMPTY; MAX_ROWS];
    let mut shown = 0;
    let mut count = 0u32;
    loop {
        let mut line = Line::EMPTY;
        let _ = write!(line, "{:04}:{}", count, WORDS[count as usize & 7]);
        if shown < rows {
            lines[shown] = line;
            shown += 1;
        } else {
            lines.copy_within(1..rows, 0);
            lines[rows - 1] = line;
        }
        draw_log(&mut fb, &lines[..shown], &font, pitch);
        draw_label(&mut fb);

        let start = Instant::now();
        let result = display.refresh(&fb, RefreshMode::M5Fastest).await;
        if result.is_err() {
            error!("update failed");
        } else if count.is_multiple_of(8) {
            info!(
                "line {}: update in {} ms",
                count,
                start.elapsed().as_millis()
            );
        }
        count = count.wrapping_add(1);
    }
}
