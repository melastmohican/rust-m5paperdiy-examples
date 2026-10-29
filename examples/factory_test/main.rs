//! # factory_test: port of the Arduino `FactoryTest` sketch (M5Stack PaperDIY)
//!
//! A two button menu with four pages, like the sketch: the factory test checklist, Compare (time
//! each refresh mode), Gray Ramp and the Game of Life. BtnA (GPIO4) moves to the next item or
//! runs the action, BtnB (GPIO3) selects or leaves a page. The async twin is `factory_test` in
//! `rust-m5paperdiy-embassy-examples`. `ui.rs` holds the drawing and the Life rules and is the
//! same file in both crates.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example factory_test
//! ```
//!
//! ## Expected
//!
//! - Portrait menu: `Factory Test`, `Compare`, `Gray Ramp`, `Life Game`. A picks, B opens.
//! - Factory Test: Battery (3.0 to 4.4 V passes), PSRAM, SD Card (card size) and Button A and
//!   Button B turn to `[PASS]` when they pass or are pressed. B leaves, A reruns once both
//!   buttons passed. **Not ported yet:** the Wi-Fi row says so and stays failed.
//! - Compare: A picks a mode, B refreshes with it and shows the time, hold B to leave.
//! - Gray Ramp: 16 bands, A inverts, B leaves.
//! - Life Game: A pauses, a second A resumes with new cells, B leaves.
//! - Differences from the Arduino sketch: `u8g2-fonts` Free Universal Bold and Helvetica
//!   instead of FreeSans, so letters differ a little.
#![no_std]
#![no_main]

extern crate alloc;

mod ui;

use defmt::{error, info};
use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c;
use embedded_hal::spi::SpiDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{SdCard, sdcard::DummyCsPin};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Instant;
use esp_hal::time::Rate;
use esp_println as _;
use rust_m5paperdiy_examples::esp_alloc::{HEAP, MemoryCapability};
use rust_m5paperdiy_examples::{epdpi, pmic};

use epdpi::prelude::*;
use ui::{
    Button, Check, Events, Fb, Life, MENU, MODES, draw_checklist, draw_compare, draw_menu,
    draw_ramp,
};

esp_bootloader_esp_idf::esp_app_desc!();

/// Shown in the footer of every page, so a photo says which build it is.
const BUILD: &str = "epdpi blocking";

/// Both buttons, polled together.
struct Pad {
    a: Input<'static>,
    b: Input<'static>,
    state_a: Button,
    state_b: Button,
}

impl Pad {
    /// Events of both buttons since the last call. Buttons are active low.
    fn poll(&mut self) -> (Events, Events) {
        let now = now_ms();
        (
            self.state_a.update(self.a.is_low(), now),
            self.state_b.update(self.b.is_low(), now),
        )
    }
}

fn now_ms() -> u64 {
    Instant::now().duration_since_epoch().as_millis()
}

/// Refreshes the screen with `mode` and returns how long it took in milliseconds.
fn show<B: EpdParallelBus>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &Fb<'_>,
    mode: RefreshMode,
) -> u32 {
    let start = now_ms();
    if display.refresh(fb, mode).is_err() {
        error!("refresh failed");
    }
    (now_ms() - start) as u32
}

/// Battery voltage in mV from the M5PM1 (registers 0x22 and 0x23, little endian).
fn battery_mv<I: I2c>(i2c: &mut I) -> Option<u16> {
    let mut raw = [0u8; 2];
    i2c.write_read(pmic::ADDR, &[0x22], &mut raw).ok()?;
    Some(u16::from_le_bytes(raw))
}

/// Card size in MiB, or `None` when no card answers: the sketch's `SD.begin` and `cardSize`.
fn sd_card_mib<S: SpiDevice<u8>, D: DelayNs>(spi: S, delay: D) -> Option<u64> {
    let card = SdCard::new(spi, DummyCsPin, delay);
    card.num_bytes().ok().map(|bytes| bytes / (1024 * 1024))
}

fn run_checklist<B: EpdParallelBus, I: I2c, S: SpiDevice<u8>>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &mut Fb<'_>,
    pad: &mut Pad,
    i2c: &mut I,
    sd: &mut S,
    delay: &Delay,
) {
    loop {
        let mut items = [
            Check::new("Battery"),
            Check::new("Wi-Fi"),
            Check::new("PSRAM"),
            Check::new("SD Card"),
            Check::new("Button A"),
            Check::new("Button B"),
        ];
        // Battery: a plausible Li-ion voltage passes, like the sketch.
        match battery_mv(i2c) {
            Some(mv) => {
                items[0].pass = mv > 3000 && mv < 4400;
                items[0]
                    .detail
                    .set(format_args!("{:.2} V", f32::from(mv) / 1000.0));
            }
            None => items[0].detail.set(format_args!("no answer")),
        }
        items[1].detail.set(format_args!("not ported yet"));
        let psram = HEAP.free_caps(MemoryCapability::External.into());
        items[2].pass = psram > 16;
        if items[2].pass {
            items[2]
                .detail
                .set(format_args!("Free {} KiB", psram / 1024));
        } else {
            items[2].detail.set(format_args!("[FAILED]"));
        }
        match sd_card_mib(&mut *sd, Delay::new()) {
            Some(mib) => {
                items[3].pass = true;
                items[3].detail.set(format_args!("{} MiB", mib));
            }
            None => items[3].detail.set(format_args!("[FAILED]")),
        }
        items[4].detail.set(format_args!("press A"));
        items[5].detail.set(format_args!("press B"));

        draw_checklist(fb, &items);
        show(display, fb, RefreshMode::M5Quality);
        loop {
            let (a, b) = pad.poll();
            let mut dirty = false;
            if a.pressed && !items[4].pass {
                items[4].pass = true;
                items[4].detail.set(format_args!("OK"));
                dirty = true;
            } else if a.pressed {
                break; // "A: rerun"
            }
            if b.pressed {
                if !items[5].pass {
                    items[5].pass = true;
                    items[5].detail.set(format_args!("OK"));
                    dirty = true;
                } else {
                    return;
                }
            }
            if dirty {
                draw_checklist(fb, &items);
                show(display, fb, RefreshMode::M5Text);
            }
            delay.delay_millis(20);
        }
    }
}

fn run_compare<B: EpdParallelBus>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &mut Fb<'_>,
    pad: &mut Pad,
    delay: &Delay,
) {
    let mut ms = [0u32; 4];
    let mut selected = 0;
    draw_compare(fb, selected, &ms);
    show(display, fb, RefreshMode::M5Quality);
    loop {
        let (a, b) = pad.poll();
        if a.pressed {
            selected = (selected + 1) % MODES.len();
            draw_compare(fb, selected, &ms);
            show(display, fb, RefreshMode::M5Fastest);
        }
        if b.clicked {
            draw_compare(fb, selected, &ms);
            ms[selected] = show(display, fb, MODES[selected].mode);
            info!("{}: {} ms", MODES[selected].name, ms[selected]);
            draw_compare(fb, selected, &ms);
            show(display, fb, RefreshMode::M5Fastest);
        }
        if b.hold {
            return;
        }
        delay.delay_millis(20);
    }
}

fn run_ramp<B: EpdParallelBus>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &mut Fb<'_>,
    pad: &mut Pad,
    delay: &Delay,
) {
    let mut inverted = false;
    loop {
        draw_ramp(fb, inverted);
        show(display, fb, RefreshMode::M5Quality);
        loop {
            let (a, b) = pad.poll();
            if a.pressed {
                inverted = !inverted;
                break;
            }
            if b.pressed {
                return;
            }
            delay.delay_millis(20);
        }
    }
}

fn run_life<B: EpdParallelBus>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &mut Fb<'_>,
    pad: &mut Pad,
    delay: &Delay,
) {
    let mut life = Life::new(Instant::now().duration_since_epoch().as_micros() as u32);
    let mut paused = false;
    loop {
        life.draw(fb, paused);
        let took = show(display, fb, RefreshMode::M5Fast);
        info!("generation {} drawn in {} ms", life.generation, took);
        life.step();

        // Wait 300 ms, or for A while paused, like the sketch.
        let start = now_ms();
        while now_ms() - start < 300 || paused {
            let (a, b) = pad.poll();
            if b.pressed {
                return;
            }
            if a.pressed {
                if paused {
                    life.reseed();
                }
                paused = !paused;
                break;
            }
            delay.delay_millis(20);
        }
    }
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    let mut i2c = rust_m5paperdiy_examples::pmic_i2c!(p);
    let delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay.clone());

    let config = InputConfig::default().with_pull(Pull::Up);
    let mut pad = Pad {
        a: Input::new(p.GPIO4, config),
        b: Input::new(p.GPIO3, config),
        state_a: Button::new(),
        state_b: Button::new(),
    };

    // microSD on SPI2: CLK 39, MOSI 38, MISO 40, CS 47, 20 MHz like the sketch.
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
    let mut sd = ExclusiveDevice::new_no_delay(spi, cs).unwrap_or_else(|e| match e {});

    let bus = rust_m5paperdiy_examples::lcd_bus!(p, 16);
    let mut prev = alloc::vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut line = alloc::vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = alloc::vec![0xFFu8; Ed047tc1::FB_BYTES];
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

    let mut selected = 0;
    draw_menu(&mut fb, selected);
    show(&mut display, &fb, RefreshMode::M5Quality);
    info!("ready");
    loop {
        let (a, b) = pad.poll();
        if a.pressed {
            selected = (selected + 1) % MENU.len();
            draw_menu(&mut fb, selected);
            show(&mut display, &fb, RefreshMode::M5Fastest);
        }
        if b.pressed {
            match selected {
                0 => run_checklist(&mut display, &mut fb, &mut pad, &mut i2c, &mut sd, &delay),
                1 => run_compare(&mut display, &mut fb, &mut pad, &delay),
                2 => run_ramp(&mut display, &mut fb, &mut pad, &delay),
                _ => run_life(&mut display, &mut fb, &mut pad, &delay),
            }
            draw_menu(&mut fb, selected);
            show(&mut display, &fb, RefreshMode::M5Quality);
        }
        delay.delay_millis(20);
    }
}
