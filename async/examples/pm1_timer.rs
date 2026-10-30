//! # pm1_timer: async port of the Arduino `M5PM1` sketch (M5Stack PaperDIY)
//!
//! Uses the M5PM1 power chip's timer. BtnA (GPIO4) arms a power-on timer for 3 s and shuts the
//! board down, so it switches itself back on 3 s later. BtnB (GPIO3) arms a power-off timer for
//! 3 s, so the board switches itself off 3 s later. The blocking twin is `pm1_timer` in
//! `rust-m5paperdiy-examples`. Here `epdpi` runs in async mode on the `esp-rtos` Embassy executor.
//!
//! **This example powers the board off.** To start it again after BtnB, press the PWR button.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example pm1_timer
//! ```
//!
//! ## Expected
//!
//! - A page `M5PM1 TIMER` with the two buttons and what they do, portrait, `M5Text`.
//! - BtnA: the page changes to `Power OFF` and `Power ON after 3s`, half a second later the board
//!   goes off, and about 3 s after that it starts again and shows the first page. The log prints
//!   `power-on timer configured` before it dies.
//! - BtnB: the page changes to `Power OFF` and `After 3 seconds`, and 3 s later the board goes
//!   off. The screen keeps the last page, as e-paper does. The log prints
//!   `power-off timer configured`.
//! - If the M5PM1 does not answer: `M5PM1 ERROR`, `Check I2C`.
//! - Differences from the Arduino sketch: `u8g2-fonts` Free Universal Bold instead of
//!   FreeSansBold, and the timer is cleared at start-up, so one armed by an earlier run cannot
//!   fire again after a reboot.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embassy_time::{Duration, Timer};
use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use embedded_hal::i2c::I2c;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Pull};
use esp_println as _;
use rust_m5paperdiy_embassy_examples::{epdpi::prelude::*, pmic};
use u8g2_fonts::{
    FontRenderer, fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
};

esp_bootloader_esp_idf::esp_app_desc!();

/// Which build this is, so a photo says it.
const BUILD: &str = "Rust: epdpi async";

/// M5PM1 registers: the 31 bit timer count (little endian), its configuration and its reload key,
/// and the system command register.
const REG_TIM_CNT: u8 = 0x38;
const REG_TIM_CFG: u8 = 0x3C;
const REG_TIM_KEY: u8 = 0x3D;
const REG_SYS_CMD: u8 = 0x0C;
const TIM_RELOAD_KEY: u8 = 0xA5;
/// Value of `REG_SYS_CMD` that shuts the board down: the command key `0xA0` and the code `0x01`.
const SYS_CMD_SHUTDOWN: u8 = 0xA1;
/// The longest timer the chip takes, in seconds.
const TIMER_MAX_SECONDS: u32 = 214_748_364;

/// What the timer does when it runs out.
#[derive(Clone, Copy)]
enum TimerAction {
    PowerOn = 0b011,
    PowerOff = 0b100,
}

/// Arms the timer: counts `seconds` down, then does `action`. Like the library's `timerSet`.
fn timer_set<I: I2c>(i2c: &mut I, seconds: u32, action: TimerAction) -> Result<(), ()> {
    if seconds > TIMER_MAX_SECONDS {
        return Err(());
    }
    let b = seconds.to_le_bytes();
    i2c.write(pmic::ADDR, &[REG_TIM_CNT, b[0], b[1], b[2], b[3] & 0x7F])
        .map_err(|_| ())?;
    // Bit 3 starts the timer, bits 2 to 0 are the action.
    i2c.write(pmic::ADDR, &[REG_TIM_CFG, 0x08 | action as u8])
        .map_err(|_| ())?;
    i2c.write(pmic::ADDR, &[REG_TIM_KEY, TIM_RELOAD_KEY])
        .map_err(|_| ())
}

/// Stops the timer. Like the library's `timerClear`.
fn timer_clear<I: I2c>(i2c: &mut I) -> Result<(), ()> {
    i2c.write(pmic::ADDR, &[REG_TIM_CFG, 0]).map_err(|_| ())?;
    i2c.write(pmic::ADDR, &[REG_TIM_KEY, TIM_RELOAD_KEY])
        .map_err(|_| ())
}

fn text(fb: &mut impl DrawTarget<Color = Gray4>, font: &FontRenderer, s: &str, x: i32, y: i32) {
    let _ = font.render_aligned(
        format_args!("{s}"),
        Point::new(x, y),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        FontColor::Transparent(Gray4::new(0)),
        fb,
    );
}

/// `FreeSansBold24pt` and `FreeSansBold18pt` stand-ins.
fn fonts() -> (FontRenderer, FontRenderer) {
    (
        FontRenderer::new::<fonts::u8g2_font_fub35_tf>(),
        FontRenderer::new::<fonts::u8g2_font_fub25_tf>(),
    )
}

/// `BUILD`, small, at the bottom of the page.
fn draw_label(fb: &mut EpdFrameBuffer<'_, Ed047tc1>) {
    let font = FontRenderer::new::<fonts::u8g2_font_helvR14_tf>();
    let (w, h) = fb.logical_size();
    text(fb, &font, BUILD, w as i32 / 2, h as i32 - 30);
}

/// A title and a line under it, in the middle of the screen.
fn draw_message(fb: &mut EpdFrameBuffer<'_, Ed047tc1>, line1: &str, line2: &str) {
    fb.fill(0xF);
    let (big, small) = fonts();
    let (w, h) = fb.logical_size();
    let (cx, cy) = (w as i32 / 2, h as i32 / 2);
    text(fb, &big, line1, cx, cy - 30);
    text(fb, &small, line2, cx, cy + 35);
    draw_label(fb);
}

/// The first page: what the two buttons do.
fn draw_home(fb: &mut EpdFrameBuffer<'_, Ed047tc1>) {
    fb.fill(0xF);
    let (big, small) = fonts();
    let (w, h) = fb.logical_size();
    let (cx, cy) = (w as i32 / 2, h as i32 / 2);
    text(fb, &big, "M5PM1 TIMER", cx, cy - 110);
    text(fb, &small, "Btn A: power on after 3 seconds", cx, cy - 35);
    text(fb, &small, "Btn B: power off after 3 seconds", cx, cy + 20);
    text(fb, &small, "Press A or B to start the timer", cx, cy + 75);
    draw_label(fb);
}

/// Shows what is in the frame buffer, with `M5Text` like the sketch's `epd_text`.
async fn show<B: EpdParallelBus>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &EpdFrameBuffer<'_, Ed047tc1>,
) {
    if display.refresh(fb, RefreshMode::M5Text).await.is_err() {
        error!("refresh failed");
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_m5paperdiy_embassy_examples::init!();
    rust_m5paperdiy_embassy_examples::start_rtos!(p);
    let mut i2c = rust_m5paperdiy_embassy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

    let config = InputConfig::default().with_pull(Pull::Up);
    let btn_a = Input::new(p.GPIO4, config);
    let btn_b = Input::new(p.GPIO3, config);

    let bus = rust_m5paperdiy_embassy_examples::lcd_bus!(p, 16);
    let mut prev = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let (Ok(mut display), Ok(mut fb)) = (
        EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line),
        EpdFrameBuffer::<Ed047tc1>::new(&mut fb_mem),
    ) else {
        error!("buffer allocation too small");
        rust_m5paperdiy_embassy_examples::halt();
    };
    fb.set_rotation(Rotation::Deg270);
    if display.clear().await.is_err() {
        error!("initial clear failed");
    }

    if !matches!(pmic::read_id(&mut i2c), Ok(pmic::DEVICE_ID)) {
        error!("the M5PM1 did not answer");
        draw_message(&mut fb, "M5PM1 ERROR", "Check I2C");
        show(&mut display, &fb).await;
        loop {
            Timer::after(Duration::from_secs(1)).await;
        }
    }
    // A timer armed by an earlier run must not fire again after this boot.
    if timer_clear(&mut i2c).is_err() {
        error!("could not clear the timer");
    }

    draw_home(&mut fb);
    show(&mut display, &fb).await;
    info!("ready");

    let (mut a_down, mut b_down) = (false, false);
    loop {
        let (a, b) = (btn_a.is_low(), btn_b.is_low());
        if a && !a_down {
            draw_message(&mut fb, "Power OFF", "Power ON after 3s");
            show(&mut display, &fb).await;
            Timer::after(Duration::from_millis(500)).await;
            if timer_set(&mut i2c, 3, TimerAction::PowerOn).is_ok() {
                info!("power-on timer configured");
                // The command takes effect after a short delay inside the chip.
                Timer::after(Duration::from_millis(120)).await;
                if i2c
                    .write(pmic::ADDR, &[REG_SYS_CMD, SYS_CMD_SHUTDOWN])
                    .is_err()
                {
                    error!("shutdown command failed");
                }
                loop {
                    Timer::after(Duration::from_secs(1)).await;
                }
            }
            error!("power-on timer configuration failed");
            draw_message(&mut fb, "Timer ERROR", "Power-on failed");
            show(&mut display, &fb).await;
        }
        if b && !b_down {
            draw_message(&mut fb, "Power OFF", "After 3 seconds");
            show(&mut display, &fb).await;
            if timer_set(&mut i2c, 3, TimerAction::PowerOff).is_ok() {
                info!("power-off timer configured");
            } else {
                error!("power-off timer configuration failed");
                draw_message(&mut fb, "Timer ERROR", "Power-off failed");
                show(&mut display, &fb).await;
            }
        }
        (a_down, b_down) = (a, b);
        Timer::after(Duration::from_millis(20)).await;
    }
}
