//! # epub_reader: port of the Arduino `EpubReader` sketch (M5Stack PaperDIY)
//!
//! Reads `.epub` books from the root of the microSD card, like `~/Src/Arduino/M5PaperDIY/EpubReader`
//! (a port of atomic14's `diy-esp32-epub-reader`). The reader itself, the zip and XML parsing, the
//! page layout, the screens and the button logic, is in `common/`, tested on the host. This file is
//! the board: the card, the display, the two buttons, and the refresh after each press. The async
//! twin is `epub_reader` in `async/`.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example epub_reader
//! ```
//!
//! Copy `.epub` files to the root of a FAT32 card. File names may be long.
//!
//! ## Controls
//!
//! | Button | Action |
//! | --- | --- |
//! | A click | down: next book, entry or page |
//! | A hold (about 0.6 s) | up: previous book, entry or page |
//! | B click | select: open the book or chapter, or go back to the list |
//!
//! ## Expected
//!
//! - The log prints `epub_reader: Rust, blocking`, then one `book:` line per book found, sorted by
//!   title, then `press <action>: N ms to lay out and draw, N ms to refresh` after every press.
//! - A "Loading..." box, then the list of books, 5 to a screen, the chosen one boxed. Then the
//!   table of contents of the book you open, 6 entries to a screen, and then its pages.
//! - PNG and JPEG pictures in a page are drawn in 16 grays, scaled to the page; the log has a
//!   `picture <path>: Drawn { bytes }` line for each. A page with a big picture takes about 1.3 s.
//! - A page turn takes about a second. At the first or last page a box says so.
//! - A card without books shows "No books on the card", one that cannot be read shows "microSD mount failed".
//! - Differences from the Arduino reader: a serif font from `u8g2` (Times) instead of FreeSerif, no
//!   italic, typographic quotes and dashes drawn as plain ones, and no saved
//!   position. Opening a book shows the table of contents first, as the sketch does.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{SdCard, TimeSource, Timestamp};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::{Instant, Rate};
use esp_println as _;
use m5paperdiy_common::input::Buttons;
use m5paperdiy_common::reader::{Action, App, Effect, Screen};
use m5paperdiy_common::render::{Geometry, draw_notice};
use m5paperdiy_common::sd::{SdLibrary, SdStorage};
use rust_m5paperdiy_examples::{epdpi::prelude::*, pmic};

esp_bootloader_esp_idf::esp_app_desc!();

/// The card needs a time for the files it writes; this reader writes none.
struct FixedTime;

impl TimeSource for FixedTime {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 9,
            zero_indexed_day: 30,
            hours: 12,
            minutes: 0,
            seconds: 0,
        }
    }
}

fn millis() -> u64 {
    Instant::now().duration_since_epoch().as_millis()
}

fn action_name(action: Action) -> &'static str {
    match action {
        Action::Up => "up",
        Action::Down => "down",
        Action::Select => "select",
    }
}

/// Shows a notice box on top of what is on the screen, refreshing only the box.
fn notice<D: EpdParallelBus>(
    display: &mut EpdDisplay<'_, D, Ed047tc1>,
    fb: &mut EpdFrameBuffer<'_, Ed047tc1>,
    lines: &[&str],
) {
    fb.clear_dirty();
    draw_notice(fb, &Geometry::PORTRAIT, lines);
    if display.refresh_dirty(fb, RefreshMode::M5Fast).is_err() {
        error!("notice refresh failed");
    }
}

/// Shows a notice that stays: the whole screen white, the box, a full refresh. For the card errors.
fn fatal<D: EpdParallelBus>(
    display: &mut EpdDisplay<'_, D, Ed047tc1>,
    fb: &mut EpdFrameBuffer<'_, Ed047tc1>,
    lines: &[&str],
) -> ! {
    let _ = fb.clear(Gray4::WHITE);
    draw_notice(fb, &Geometry::PORTRAIT, lines);
    if display.refresh(fb, RefreshMode::M5Text).is_err() {
        error!("refresh failed");
    }
    rust_m5paperdiy_examples::halt();
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    info!("epub_reader: Rust, blocking");
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut Delay::new());

    let config = InputConfig::default().with_pull(Pull::Up);
    let btn_a = Input::new(p.GPIO4, config);
    let btn_b = Input::new(p.GPIO3, config);

    let spi = Spi::new(
        p.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(20))
            .with_mode(SpiMode::_0),
    )
    .map(|spi| {
        spi.with_sck(p.GPIO39)
            .with_miso(p.GPIO40)
            .with_mosi(p.GPIO38)
    });
    let Ok(spi) = spi else {
        error!("spi config failed");
        rust_m5paperdiy_examples::halt();
    };
    let cs = Output::new(p.GPIO47, Level::High, OutputConfig::default());
    // Infallible for an `Output` chip select.
    let device = ExclusiveDevice::new_no_delay(spi, cs).unwrap_or_else(|e| match e {});
    let card = SdCard::new(device, Delay::new());

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
    // `setRotation(0)` of the sketch: portrait, 540 x 960.
    fb.set_rotation(Rotation::Deg270);
    if display.clear().is_err() {
        error!("initial clear failed");
    }

    // Looking through the card takes a moment: it opens every book to read its title.
    notice(&mut display, &mut fb, &["Loading..."]);
    let storage = match SdStorage::new(card, FixedTime) {
        Ok(storage) => storage,
        Err(_) => {
            error!("microSD initialization failed");
            fatal(
                &mut display,
                &mut fb,
                &["microSD mount failed", "Check card / FAT32"],
            );
        }
    };
    let started = millis();
    let mut library = match SdLibrary::new(storage, Geometry::PORTRAIT) {
        Ok(library) => library,
        Err(_) => {
            error!("microSD directory could not be read");
            fatal(
                &mut display,
                &mut fb,
                &["microSD mount failed", "Check card / FAT32"],
            );
        }
    };
    info!(
        "{} books, found in {} ms",
        library.books().len(),
        millis() - started
    );
    for index in 0..library.books().len() {
        let book = &library.books()[index];
        info!("book: {} ({})", book.title.as_str(), book.path.as_str());
    }

    // The first screen, drawn with the full waveform.
    let mut app = App::new();
    let _ = fb.clear(Gray4::WHITE);
    library.draw(&app, Effect::Redraw, &mut fb);
    if display.refresh(&fb, RefreshMode::M5Quality).is_err() {
        error!("first refresh failed");
    }

    let mut buttons = Buttons::new(btn_a.is_low(), btn_b.is_low());
    loop {
        if let Some(action) = buttons.update(btn_a.is_low(), btn_b.is_low(), millis()) {
            // Opening a book and the first page of a chapter take a while.
            if action == Action::Select && app.screen() != Screen::Reading {
                notice(&mut display, &mut fb, &["Loading..."]);
            }
            let started = millis();
            let effect = library.press(&mut app, action, &mut fb);
            let drawn = millis();
            for (path, status) in library.last_pictures() {
                info!("picture {}: {}", path.as_str(), defmt::Debug2Format(status));
            }
            if effect != Effect::None {
                if display.refresh(&fb, RefreshMode::M5Text).is_err() {
                    error!("refresh failed");
                }
                info!(
                    "press {}: {} ms to lay out and draw, {} ms to refresh",
                    action_name(action),
                    drawn - started,
                    millis() - drawn
                );
            }
        }
        delay.delay_millis(20);
    }
}
