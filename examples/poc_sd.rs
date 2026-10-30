//! # poc_sd: can Rust read the microSD card? (M5Stack PaperDIY)
//!
//! Proof of concept for the `sd_images` and `sd_speed` ports and the `factory_test`
//! checklist. Opens the card over SPI at 20 MHz like the sketch (CLK 39, MOSI 38, MISO 40, CS 47, the pins in the
//! Arduino `FactoryTest`) with `embedded-sdmmc`, then:
//!
//! 1. prints the card size, as the checklist does (`SD.cardSize()`);
//! 2. lists the root directory of the first FAT volume;
//! 3. reads the biggest file it found, up to 512 KiB, and prints the speed in KiB/s.
//!
//! No display. Blocking only.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example poc_sd
//! ```
//!
//! ## Expected
//!
//! - `card N MiB`, then one line per root entry, then `read N bytes in N ms (N KiB/s)`.
//! - Put `PaperDIY_01.png` and `PaperDIY_02.png` from `~/Src/Arduino/M5PaperDIY` on a FAT32
//!   card to get files to list and read.
#![no_std]
#![no_main]

use core::fmt::Write;
use defmt::{error, info};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::sdcard::DummyCsPin;
use embedded_sdmmc::{Mode, SdCard, TimeSource, Timestamp, VolumeIdx, VolumeManager};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::{Instant, Rate};
use esp_println as _;

esp_bootloader_esp_idf::esp_app_desc!();

/// No clock on the board, so files get a fixed date.
struct FixedTime;

impl TimeSource for FixedTime {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 9,
            zero_indexed_day: 28,
            hours: 12,
            minutes: 0,
            seconds: 0,
        }
    }
}

/// A short file name as text, 8.3 so 12 bytes plus the dot is plenty.
struct Name {
    buf: [u8; 16],
    len: usize,
}

impl Write for Name {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len + s.len();
        let dst = self.buf.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        dst.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[esp_hal::main]
fn main() -> ! {
    let p = rust_m5paperdiy_examples::init!();
    info!("poc_sd: Rust, blocking");
    let delay = Delay::new();

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
    // Infallible for a `Output` chip select.
    let device = ExclusiveDevice::new_no_delay(spi, cs).unwrap_or_else(|e| match e {});

    let card = SdCard::new(device, DummyCsPin, delay);
    match card.num_bytes() {
        Ok(bytes) => info!("card {} MiB", bytes / (1024 * 1024)),
        Err(_) => {
            error!("no card answered, check it is in and FAT32");
            rust_m5paperdiy_examples::halt();
        }
    }

    let mut volumes = VolumeManager::new(card, FixedTime);
    let Ok(mut volume) = volumes.open_volume(VolumeIdx(0)) else {
        error!("no FAT volume on the card");
        rust_m5paperdiy_examples::halt();
    };
    let Ok(mut root) = volume.open_root_dir() else {
        error!("cannot open the root directory");
        rust_m5paperdiy_examples::halt();
    };

    // List the root and remember the biggest file.
    let mut biggest = Name {
        buf: [0; 16],
        len: 0,
    };
    let mut biggest_size = 0u32;
    let listed = root.iterate_dir(|entry| {
        info!(
            "  {} ({} bytes{})",
            defmt::Display2Format(&entry.name),
            entry.size,
            if entry.attributes.is_directory() {
                ", dir"
            } else {
                ""
            }
        );
        if !entry.attributes.is_directory() && entry.size > biggest_size {
            let mut name = Name {
                buf: [0; 16],
                len: 0,
            };
            if write!(name, "{}", entry.name).is_ok() {
                biggest = name;
                biggest_size = entry.size;
            }
        }
    });
    if listed.is_err() {
        error!("listing failed");
    }

    let Ok(name) = core::str::from_utf8(&biggest.buf[..biggest.len]) else {
        info!("no file to read");
        rust_m5paperdiy_examples::halt();
    };
    if name.is_empty() {
        info!("no file to read, put PaperDIY_01.png on the card");
        rust_m5paperdiy_examples::halt();
    }
    let Ok(mut file) = root.open_file_in_dir(name, Mode::ReadOnly) else {
        error!("cannot open {}", name);
        rust_m5paperdiy_examples::halt();
    };
    let mut buf = [0u8; 4096];
    let mut total = 0usize;
    let start = Instant::now();
    while total < 512 * 1024 {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(_) => {
                error!("read failed after {} bytes", total);
                break;
            }
        }
    }
    let ms = start.elapsed().as_millis().max(1);
    info!(
        "read {} bytes of {} in {} ms ({} KiB/s)",
        total,
        name,
        ms,
        total as u64 * 1000 / 1024 / ms
    );
    info!("done");
    loop {
        delay.delay_millis(1000);
    }
}
