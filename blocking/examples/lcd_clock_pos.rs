//! # lcd_clock_pos: where in a row is the extra LCD_CAM clock? (M5Stack PaperDIY)
//!
//! Backlog `lcd-line-start-check` (`epdpi`). `lcd_clock_count` found one more CL cycle than bytes in every
//! row of `LcdBus`. This finds out whether it comes before or after the data, by counting CL edges while the
//! data line D0 (GPIO6, read back from the pad) is high or low, for rows where D0 is high in one place only.
//! The panel is not powered, nothing shows on the glass. Rows have 248 bytes.
//!
//! Patterns (byte 0x01 has D0 high, 0x00 low): `none` all low, `first` byte 0 high, `middle` byte 124 high,
//! `last` byte 247 high, `all` every byte high.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example lcd_clock_pos
//! ```
//!
//! ## Expected
//!
//! The log repeats every few seconds. For each pattern and each of four counters (CL rising or falling edge,
//! D0 high or low) it prints how many rows had each count. 249 edges per row in all. Read it like this:
//!
//! - `none`: high counts 0, low counts 249. If not, the counting is wrong and nothing below counts.
//! - `all`: high 249 means D0 is already high at the extra clock (the first byte is on the bus before the
//!   clock, or the byte stays after the last one). High 248 means the bus is low at the extra clock.
//! - `first` high 2 and `last` high 1: the extra clock is before the data and sees byte 0 on the bus.
//!   `first` 1 and `last` 2: the extra clock is after the data and sees the last byte still on the bus.
//!   `first` 1 and `last` 1: the bus is idle (low) at the extra clock, so it is before the first byte
//!   or after the last, the `all` row and the clock phase of the edges tell which.
//! - Rising and falling counters disagree on a row, or counts that differ from row to row: D0 changes at
//!   the clock edge, the counter races, trust only the edge type that gives stable counts.
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
const LEN: usize = 248;

#[esp_hal::main]
fn main() -> ! {
    use esp_hal::dma_tx_buffer;
    use esp_hal::lcd_cam::LcdCam;
    use esp_hal::lcd_cam::lcd::i8080::{Config, I8080};
    use esp_hal::time::Rate;

    let p = rust_m5paperdiy_examples::init!();
    let delay = Delay::new();

    // CL, SPH and D0 are driven by LCD_CAM. The input buffer of their pads is switched on so that PCNT can
    // read what the pins do.
    let mut cl = Flex::new(p.GPIO16);
    let mut cs = Flex::new(p.GPIO13);
    let mut d0 = Flex::new(p.GPIO6);
    cl.set_input_enable(true);
    cs.set_input_enable(true);
    d0.set_input_enable(true);
    let cl_in = cl.peripheral_input();
    let d0_in = d0.peripheral_input();
    let d0_level = d0.peripheral_input();

    let lcd_cam = LcdCam::new(p.LCD_CAM);
    let config = Config::default().with_frequency(Rate::from_mhz(16));
    let Ok(i8080) = I8080::new(lcd_cam.lcd, p.DMA_CH0, config) else {
        panic!("LCD_CAM config rejected");
    };
    let i8080 = i8080
        .with_wrx(cl)
        .with_cs(cs)
        .with_data0(d0)
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
    // The edge is CL, the control signal is D0: the counter only counts while D0 has the wanted level.
    // unit 0: CL rising, D0 high. unit 1: CL falling, D0 high. unit 2: CL rising, D0 low. unit 3: CL falling, D0 low.
    let rise_hi = &pcnt.unit0;
    rise_hi.channel0.set_edge_signal(cl_in.clone());
    rise_hi.channel0.set_ctrl_signal(d0_in.clone());
    rise_hi
        .channel0
        .set_ctrl_mode(CtrlMode::Disable, CtrlMode::Keep);
    rise_hi
        .channel0
        .set_input_mode(EdgeMode::Hold, EdgeMode::Increment);
    let fall_hi = &pcnt.unit1;
    fall_hi.channel0.set_edge_signal(cl_in.clone());
    fall_hi.channel0.set_ctrl_signal(d0_in.clone());
    fall_hi
        .channel0
        .set_ctrl_mode(CtrlMode::Disable, CtrlMode::Keep);
    fall_hi
        .channel0
        .set_input_mode(EdgeMode::Increment, EdgeMode::Hold);
    let rise_lo = &pcnt.unit2;
    rise_lo.channel0.set_edge_signal(cl_in.clone());
    rise_lo.channel0.set_ctrl_signal(d0_in.clone());
    rise_lo
        .channel0
        .set_ctrl_mode(CtrlMode::Keep, CtrlMode::Disable);
    rise_lo
        .channel0
        .set_input_mode(EdgeMode::Hold, EdgeMode::Increment);
    let fall_lo = &pcnt.unit3;
    fall_lo.channel0.set_edge_signal(cl_in);
    fall_lo.channel0.set_ctrl_signal(d0_in);
    fall_lo
        .channel0
        .set_ctrl_mode(CtrlMode::Keep, CtrlMode::Disable);
    fall_lo
        .channel0
        .set_input_mode(EdgeMode::Increment, EdgeMode::Hold);
    rise_hi.set_filter(None).ok();
    fall_hi.set_filter(None).ok();
    rise_lo.set_filter(None).ok();
    fall_lo.set_filter(None).ok();

    // The report repeats every few seconds, so a serial monitor that starts late still sees it.
    loop {
        for (name, high_at) in [
            ("none", None),
            ("first", Some(0usize)),
            ("middle", Some(124)),
            ("last", Some(LEN - 1)),
            ("all", Some(usize::MAX)),
        ] {
            let mut row = vec![0u8; LEN];
            match high_at {
                None => {}
                Some(usize::MAX) => row.fill(1),
                Some(i) => row[i] = 1,
            }
            let mut c_rh = [0u16; ROWS];
            let mut c_fh = [0u16; ROWS];
            let mut c_rl = [0u16; ROWS];
            let mut c_fl = [0u16; ROWS];
            let _ = bus.start_frame();
            for r in 0..ROWS {
                rise_hi.clear();
                fall_hi.clear();
                rise_lo.clear();
                fall_lo.clear();
                let _ = bus.write_scanline(&row);
                c_rh[r] = rise_hi.value() as u16;
                c_fh[r] = fall_hi.value() as u16;
                c_rl[r] = rise_lo.value() as u16;
                c_fl[r] = fall_lo.value() as u16;
                let _ = bus.next_row();
            }
            let _ = bus.end_frame();
            // Level of D0 after the last row: does the bus keep the last byte or fall back to idle?
            println!(
                "{} D0 level after the last row: {}",
                name,
                d0_level.is_input_high()
            );
            report(name, "rising  D0 high", &c_rh);
            report(name, "falling D0 high", &c_fh);
            report(name, "rising  D0 low ", &c_rl);
            report(name, "falling D0 low ", &c_fl);
        }
        println!("done");
        delay.delay_millis(3000);
    }
}

/// Prints how many rows had each count (at most 4 distinct values, then the rest as "other").
fn report(pattern: &str, what: &str, counts: &[u16; ROWS]) {
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
    let mut line = alloc::string::String::new();
    for i in 0..used {
        use core::fmt::Write;
        let _ = write!(line, " {}x{}", vals[i], hits[i]);
    }
    println!(
        "{} {}: count x rows:{} other {}",
        pattern, what, line, other
    );
}
