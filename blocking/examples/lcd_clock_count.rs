//! # lcd_clock_count: does `LcdBus` clock exactly one CL edge per byte of a row? (M5Stack PaperDIY)
//!
//! Backlog `lcd-line-start-check` (`epdpi`), stage 1. FastEPD documents a spurious clock before the data on
//! the ESP32-S3 LCD hardware. This counts rising edges of CL (GPIO16) with the PCNT peripheral, read back
//! from the same pad, in four ways: rising inside a row (SPH, GPIO13, low), falling inside a row, rising in all, and the falling
//! edges of SPH (rows), and the falling edges of CL inside a row. It sends 540 rows of 248 bytes (`Quality16` row, 240 data bytes and 8 padding) and
//! then 540 rows of 256 bytes. The panel is not powered, nothing shows on the glass.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example lcd_clock_count
//! ```
//!
//! ## Expected
//!
//! The log prints, in a loop every few seconds, for each row length, how many rows had each count. PASS: every row counts exactly its
//! length inside the row (248, then 256), rising and falling, and the rising count of all edges is the same.
//! A count of length + 1 inside the row is a spurious clock.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use epdpi::EpdParallelBus;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Flex, Level, Output, OutputConfig};
use esp_hal::pcnt::{
    Pcnt,
    channel::{CtrlMode, EdgeMode},
};
use esp_println::println;

esp_bootloader_esp_idf::esp_app_desc!();

const ROWS: usize = 540;

#[esp_hal::main]
fn main() -> ! {
    use esp_hal::dma_tx_buffer;
    use esp_hal::lcd_cam::LcdCam;
    use esp_hal::lcd_cam::lcd::i8080::{Config, I8080};
    use esp_hal::time::Rate;

    let p = rust_m5paperdiy_examples::init!();
    let delay = Delay::new();

    // CL and SPH are driven by LCD_CAM. The input buffer of both pads is switched on so that PCNT can
    // read what the pins do.
    let mut cl = Flex::new(p.GPIO16);
    let mut cs = Flex::new(p.GPIO13);
    cl.set_input_enable(true);
    cs.set_input_enable(true);
    println!(
        "idle level before start: CL {} SPH {}",
        cl.is_high(),
        cs.is_high()
    );
    let cl_in = cl.peripheral_input();
    let cs_in = cs.peripheral_input();

    let lcd_cam = LcdCam::new(p.LCD_CAM);
    let config = Config::default().with_frequency(Rate::from_mhz(16));
    let Ok(i8080) = I8080::new(lcd_cam.lcd, p.DMA_CH0, config) else {
        panic!("LCD_CAM config rejected");
    };
    let i8080 = i8080
        .with_wrx(cl)
        .with_cs(cs)
        .with_data0(p.GPIO6)
        .with_data1(p.GPIO14)
        .with_data2(p.GPIO7)
        .with_data3(p.GPIO12)
        .with_data4(p.GPIO9)
        .with_data5(p.GPIO11)
        .with_data6(p.GPIO8)
        .with_data7(p.GPIO10);
    let Ok(buf) = dma_tx_buffer!(256) else {
        panic!("DMA buffer allocation failed");
    };
    let out =
        |pin: esp_hal::gpio::AnyPin<'static>| Output::new(pin, Level::Low, OutputConfig::default());
    let mut bus = epdpi_esp::lcd_bus::LcdBus::new(
        i8080,
        buf,
        epdpi_esp::lcd_bus::LcdControlPins {
            ckv: out(p.GPIO18.into()),
            spv: out(p.GPIO17.into()),
            oe: out(p.GPIO45.into()),
            le: out(p.GPIO15.into()),
            pwr: out(p.GPIO46.into()),
        },
        Delay::new(),
    );

    let pcnt = Pcnt::new(p.PCNT);
    // unit 0: CL rising edges while SPH is low (inside a row)
    let inside = &pcnt.unit0;
    inside.channel0.set_edge_signal(cl_in.clone());
    inside.channel0.set_ctrl_signal(cs_in.clone());
    inside
        .channel0
        .set_ctrl_mode(CtrlMode::Keep, CtrlMode::Disable);
    inside
        .channel0
        .set_input_mode(EdgeMode::Hold, EdgeMode::Increment);
    // unit 1: CL falling edges while SPH is low (inside a row)
    let falling = &pcnt.unit1;
    falling.channel0.set_edge_signal(cl_in.clone());
    falling.channel0.set_ctrl_signal(cs_in.clone());
    falling
        .channel0
        .set_ctrl_mode(CtrlMode::Keep, CtrlMode::Disable);
    falling
        .channel0
        .set_input_mode(EdgeMode::Increment, EdgeMode::Hold);
    // unit 2: all CL rising edges
    let all = &pcnt.unit2;
    all.channel0.set_edge_signal(cl_in);
    all.channel0.set_ctrl_mode(CtrlMode::Keep, CtrlMode::Keep);
    all.channel0
        .set_input_mode(EdgeMode::Hold, EdgeMode::Increment);
    // unit 3: falling edges of SPH (one per row)
    let rows = &pcnt.unit3;
    rows.channel0.set_edge_signal(cs_in);
    rows.channel0.set_ctrl_mode(CtrlMode::Keep, CtrlMode::Keep);
    rows.channel0
        .set_input_mode(EdgeMode::Increment, EdgeMode::Hold);
    inside.set_filter(None).ok();
    falling.set_filter(None).ok();
    all.set_filter(None).ok();
    rows.set_filter(None).ok();
    inside.clear();
    falling.clear();
    all.clear();
    rows.clear();

    // The report repeats every few seconds, so a serial monitor that starts late still sees it.
    loop {
        for len in [248usize, 256] {
            let mut row = vec![0u8; len];
            for (i, b) in row.iter_mut().enumerate() {
                *b = (i as u8).wrapping_mul(37);
            }
            let mut n_inside = [0u16; ROWS];
            let mut n_fall = [0u16; ROWS];
            let mut n_all = [0u16; ROWS];
            let mut n_rows = [0u16; ROWS];
            let _ = bus.start_frame();
            for r in 0..ROWS {
                inside.clear();
                falling.clear();
                all.clear();
                rows.clear();
                let _ = bus.write_scanline(&row);
                n_inside[r] = inside.value() as u16;
                n_fall[r] = falling.value() as u16;
                n_all[r] = all.value() as u16;
                n_rows[r] = rows.value() as u16;
                let _ = bus.next_row();
            }
            let _ = bus.end_frame();
            report(len, "inside a row", &n_inside, len as u16);
            report(len, "CL falling edges inside a row", &n_fall, len as u16);
            report(len, "all CL edges", &n_all, len as u16);
            report(len, "SPH falling edges", &n_rows, 1);
        }
        println!("done");
        delay.delay_millis(3000);
    }
}

/// Prints how many rows had each count (at most 4 distinct values) and whether all equal `expect`.
fn report(len: usize, what: &str, counts: &[u16; ROWS], expect: u16) {
    let mut vals = [0u16; 4];
    let mut hits = [0usize; 4];
    let mut used = 0;
    let mut other = 0;
    for &c in counts {
        match vals[..used].iter().position(|&v| v == c) {
            Some(i) => hits[i] += 1,
            None if used < 4 => {
                vals[used] = c;
                hits[used] = 1;
                used += 1;
            }
            None => other += 1,
        }
    }
    println!("len {} {}: expect {}", len, what, expect);
    for i in 0..used {
        println!("    count {} on {} rows", vals[i], hits[i]);
    }
    if other > 0 {
        println!("    {} rows with other counts", other);
    }
}
