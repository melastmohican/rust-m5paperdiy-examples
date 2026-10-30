//! # sd_images: async port of the Arduino `microSD` sketch (M5Stack PaperDIY)
//!
//! Mounts the microSD card, appends a line to a log file and prints it back, then shows a PNG from
//! the card. BtnA (GPIO4) shows the previous image and BtnB (GPIO3) the next one, two in all.
//! The images are decoded to 960 by 540 landscape and shown with `RefreshMode::M5Quality`. The
//! blocking twin is `sd_images` in `rust-m5paperdiy-examples`. Here `epdpi` runs in async mode on the
//! `esp-rtos` Embassy executor; the card is still read with blocking SPI.
//!
//! ## Run
//!
//! Copy `PaperDIY_01.png` and `PaperDIY_02.png` from `~/Src/Arduino/M5PaperDIY` to the root of a
//! FAT32 card, put it in the board, then:
//!
//! ```text
//! cargo run --release --example sd_images
//! ```
//!
//! ## Expected
//!
//! - The screen says `PaperDIY microSD` and `Initializing card`, then the first image. The log
//!   prints `Card size: N MB`, `Wrote PAPERLOG.TXT`, the file contents and `Displayed: ...`.
//! - BtnA and BtnB flip between the two images (about 5 s each: the file is about 1 MB, read at
//!   about 0.5 MB/s, then decoded and refreshed). Pressing during a load is seen after it, as in the sketch.
//! - No card or not FAT32: `microSD mount failed`, `Check card / FAT32`. A missing image: `PNG not
//!   found`, a file that does not decode: `PNG load failed`.
//! - Differences from the Arduino sketch: `embedded-sdmmc` has no long file names, so the images
//!   are the first two `.PNG` files in the root, those whose short name starts with `PAPER` first
//!   (`PaperDIY_01.png` shows up as `PAPER~26.PNG`, in name order). The log is `PAPERLOG.TXT`, not
//!   `PaperDIY_Log.txt`. The small `Rust: epdpi async` tag at the bottom left of each image
//!   says which build a photo shows; the sketch has none.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use defmt::{error, info};
use embassy_time::{Duration, Timer};
use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{BlockDevice, SdCard, TimeSource, Timestamp};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::{Instant, Rate};
use esp_println as _;
use rust_m5paperdiy_embassy_examples::{epdpi, epdpi::prelude::*, pmic};

esp_bootloader_esp_idf::esp_app_desc!();

/// Which build this is, so a photo says it.
const BUILD: &str = "Rust: epdpi async";

/// Hardware-free part: the card access, the picture and the status screens. Same text in the
/// async crate, and tested on the host by `host-tests`.
mod ui {
    use alloc::vec::Vec;
    use core::fmt::Write;
    use core::ops::ControlFlow;
    use embedded_graphics::{pixelcolor::Gray4, prelude::*};
    use embedded_sdmmc::{
        BlockDevice, Error, Mode, RawDirectory, TimeSource, VolumeIdx, VolumeManager,
    };
    use u8g2_fonts::{
        FontRenderer, fonts,
        types::{FontColor, HorizontalAlignment, VerticalPosition},
    };

    use crate::epdpi::prelude::*;

    pub type Fb<'a> = EpdFrameBuffer<'a, Ed047tc1>;

    /// The images of the sketch, by their long names. The card is searched for them by short name.
    pub const IMAGE_NAMES: [&str; 2] = ["/PaperDIY_01.png", "/PaperDIY_02.png"];
    /// The sketch's log file, `/PaperDIY_Log.txt`, as a short name.
    pub const LOG_NAME: &str = "PAPERLOG.TXT";
    pub const LOG_LINE: &[u8] = b"PaperDIY microSD test\r\n";

    /// A short (8.3) file name as text.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct ShortName {
        buf: [u8; 12],
        len: usize,
    }

    impl ShortName {
        pub const EMPTY: Self = Self {
            buf: [0; 12],
            len: 0,
        };

        pub fn as_str(&self) -> &str {
            core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
        }

        /// Sort key: names starting with `PAPER` come first, then by name.
        fn rank(&self) -> (bool, &str) {
            (!self.as_str().starts_with("PAPER"), self.as_str())
        }
    }

    impl Write for ShortName {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let end = self.len + s.len();
            let dst = self.buf.get_mut(self.len..end).ok_or(core::fmt::Error)?;
            dst.copy_from_slice(s.as_bytes());
            self.len = end;
            Ok(())
        }
    }

    /// Why the card could not give a file.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum StoreError {
        /// No card, or no FAT volume on it.
        Mount,
        NotFound,
        NoMemory,
        Io,
    }

    fn io<E: core::error::Error>(error: Error<E>) -> StoreError {
        match error {
            Error::NotFound => StoreError::NotFound,
            Error::DeviceError(_) | Error::NoSuchVolume | Error::FormatError(_) => {
                StoreError::Mount
            }
            _ => StoreError::Io,
        }
    }

    /// The card, with the operations the sketch uses.
    pub struct Store<D: BlockDevice, T: TimeSource> {
        mgr: VolumeManager<D, T>,
    }

    impl<D: BlockDevice, T: TimeSource> Store<D, T> {
        pub fn new(device: D, time: T) -> Self {
            Self {
                mgr: VolumeManager::new(device, time),
            }
        }

        /// Size of the card in bytes, `None` when it does not answer.
        pub fn card_bytes(&mut self) -> Option<u64> {
            self.mgr
                .device(|card| card.num_blocks())
                .ok()
                .map(|blocks| u64::from(blocks.0) * 512)
        }

        /// Runs `f` on the root directory of the first volume and closes both afterwards.
        fn with_root<R>(
            &mut self,
            f: impl FnOnce(&mut VolumeManager<D, T>, RawDirectory) -> Result<R, StoreError>,
        ) -> Result<R, StoreError> {
            let volume = self
                .mgr
                .open_raw_volume(VolumeIdx(0))
                .map_err(|_| StoreError::Mount)?;
            let dir = match self.mgr.open_root_dir(volume) {
                Ok(dir) => dir,
                Err(_) => {
                    let _ = self.mgr.close_volume(volume);
                    return Err(StoreError::Mount);
                }
            };
            let result = f(&mut self.mgr, dir);
            let _ = self.mgr.close_dir(dir);
            let _ = self.mgr.close_volume(volume);
            result
        }

        /// The sketch's log step: appends a line to the log file.
        pub fn append_log(&mut self) -> Result<(), StoreError> {
            self.with_root(|mgr, dir| {
                let file = mgr
                    .open_file_in_dir(dir, LOG_NAME, Mode::ReadWriteCreateOrAppend)
                    .map_err(io)?;
                let written = mgr.write(file, LOG_LINE).map_err(io);
                let closed = mgr.close_file(file).map_err(io);
                written.and(closed)
            })
        }

        /// The whole log file.
        pub fn read_log(&mut self) -> Result<Vec<u8>, StoreError> {
            self.with_root(|mgr, dir| read_all(mgr, dir, LOG_NAME))
        }

        /// The two image slots: the first two `.PNG` files of the root, names starting with `PAPER`
        /// first. Hidden files (macOS leaves `._` copies on FAT cards) and folders are skipped.
        pub fn find_images(&mut self) -> Result<[Option<ShortName>; 2], StoreError> {
            self.with_root(|mgr, dir| {
                let mut slots: [Option<ShortName>; 2] = [None, None];
                mgr.iterate_dir(dir, |entry| {
                    if entry.attributes.is_directory()
                        || entry.attributes.is_hidden()
                        || entry.attributes.is_volume()
                        || entry.size == 0
                        || entry.name.extension() != b"PNG"
                    {
                        return ControlFlow::Continue(());
                    }
                    let mut name = ShortName::EMPTY;
                    if write!(name, "{}", entry.name).is_err() {
                        return ControlFlow::Continue(());
                    }
                    // Keep the two best, in order.
                    let mut incoming = name;
                    for slot in slots.iter_mut() {
                        match slot {
                            Some(have) if have.rank() <= incoming.rank() => {}
                            _ => {
                                if let Some(have) = slot.replace(incoming) {
                                    incoming = have;
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                    ControlFlow::Continue(())
                })
                .map_err(io)?;
                Ok(slots)
            })
        }

        /// A file from the root, whole.
        pub fn read_file(&mut self, name: &str) -> Result<Vec<u8>, StoreError> {
            self.with_root(|mgr, dir| read_all(mgr, dir, name))
        }
    }

    fn read_all<D: BlockDevice, T: TimeSource>(
        mgr: &mut VolumeManager<D, T>,
        dir: RawDirectory,
        name: &str,
    ) -> Result<Vec<u8>, StoreError> {
        let file = mgr
            .open_file_in_dir(dir, name, Mode::ReadOnly)
            .map_err(io)?;
        let result = (|| {
            let length = mgr.file_length(file).map_err(io)? as usize;
            let mut data = Vec::new();
            data.try_reserve_exact(length)
                .map_err(|_| StoreError::NoMemory)?;
            data.resize(length, 0);
            let mut have = 0;
            while have < length {
                match mgr.read(file, &mut data[have..]).map_err(io)? {
                    0 => return Err(StoreError::Io),
                    n => have += n,
                }
            }
            Ok(data)
        })();
        let _ = mgr.close_file(file);
        result
    }

    /// Which image is up. A goes back and B forward, both wrap, like the sketch.
    #[derive(Clone, Copy)]
    pub struct Pager {
        count: usize,
        current: usize,
    }

    impl Pager {
        pub fn new(count: usize) -> Self {
            Self { count, current: 0 }
        }

        pub fn current(&self) -> usize {
            self.current
        }

        pub fn previous(&mut self) -> usize {
            self.current = (self.current + self.count - 1) % self.count;
            self.current
        }

        pub fn next(&mut self) -> usize {
            self.current = (self.current + 1) % self.count;
            self.current
        }
    }

    /// M5GFX's ordered dither matrix (`Panel_EPD.inl`), indexed by `(y & 3) * 4 + (x & 3)`.
    const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

    /// 8 bit gray to the panel's 16 levels the way M5GFX's `writeImage` does it.
    pub fn quantize(value: u8, x: u32, y: u32) -> u8 {
        let bayer = i32::from(BAYER[((y & 3) * 4 + (x & 3)) as usize]);
        ((i32::from(value) + bayer - 8) >> 4).clamp(0, 15) as u8
    }

    /// Luma of an RGBA pixel over the white the sketch fills the screen with.
    pub fn gray_over_white(r: u8, g: u8, b: u8, a: u8) -> u8 {
        let luma = (u32::from(r) * 77 + u32::from(g) * 151 + u32::from(b) * 28) >> 8;
        let a = u32::from(a);
        ((luma * a + 255 * (255 - a) + 127) / 255).min(255) as u8
    }

    /// Draws a `width` by `height` RGBA image (4 bytes a pixel) at the top left. What does not fit
    /// is cut.
    pub fn draw_rgba(fb: &mut Fb<'_>, rgba: &[u8], width: u32, height: u32) {
        let (w, h) = fb.logical_size();
        for y in 0..height.min(h) {
            let row = &rgba[(y * width) as usize * 4..((y + 1) * width) as usize * 4];
            for (x, px) in row.chunks_exact(4).enumerate().take(w as usize) {
                let value = gray_over_white(px[0], px[1], px[2], px[3]);
                fb.set_logical_pixel(x as u32, y, quantize(value, x as u32, y));
            }
        }
    }

    /// The sketch's `showStatus`: two centered lines on white, the title 35 px above the middle
    /// and the detail 35 px below.
    pub fn draw_status(fb: &mut Fb<'_>, title: &str, detail: &str, label: &str) {
        let (w, h) = fb.logical_size();
        let _ = fb.clear(Gray4::WHITE);
        let big = FontRenderer::new::<fonts::u8g2_font_fub35_tf>();
        let small = FontRenderer::new::<fonts::u8g2_font_fub25_tf>();
        let tiny = FontRenderer::new::<fonts::u8g2_font_fub17_tf>();
        for (font, text, y) in [
            (&big, title, h as i32 / 2 - 35),
            (&small, detail, h as i32 / 2 + 35),
            // Not in the sketch: says which build a photo shows.
            (&tiny, label, h as i32 - 40),
        ] {
            let _ = font.render_aligned(
                format_args!("{text}"),
                Point::new(w as i32 / 2, y),
                VerticalPosition::Center,
                HorizontalAlignment::Center,
                FontColor::Transparent(Gray4::BLACK),
                fb,
            );
        }
    }

    /// A small tag at the bottom left of an image, on white, to say which build this is.
    pub fn draw_label(fb: &mut Fb<'_>, label: &str) {
        let font = FontRenderer::new::<fonts::u8g2_font_helvR12_tf>();
        let (_, h) = fb.logical_size();
        let top = h as i32 - 26;
        for y in top..h as i32 {
            for x in 0..200 {
                fb.set_logical_pixel(x, y as u32, 15);
            }
        }
        let _ = font.render_aligned(
            format_args!("{label}"),
            Point::new(8, top + 13),
            VerticalPosition::Center,
            HorizontalAlignment::Left,
            FontColor::Transparent(Gray4::new(0)),
            fb,
        );
    }
}

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

fn now_ms() -> u64 {
    Instant::now().duration_since_epoch().as_millis()
}

/// Decodes `png` and draws it. `Err` is the message for the `PNG load failed` screen.
fn draw_png(fb: &mut ui::Fb<'_>, png: &[u8]) -> Result<(), ()> {
    let header = minipng::decode_png_header(png).map_err(|_| ())?;
    let mut scratch = Vec::new();
    scratch
        .try_reserve_exact(header.required_bytes_rgba8bpc())
        .map_err(|_| ())?;
    scratch.resize(header.required_bytes_rgba8bpc(), 0u8);
    let mut image = minipng::decode_png(png, &mut scratch).map_err(|_| ())?;
    image.convert_to_rgba8bpc().map_err(|_| ())?;
    ui::draw_rgba(fb, image.pixels(), image.width(), image.height());
    Ok(())
}

/// A status screen, with `M5Text` like the sketch's `epd_text`.
async fn status<B: EpdParallelBus>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &mut ui::Fb<'_>,
    title: &str,
    detail: &str,
) {
    ui::draw_status(fb, title, detail, BUILD);
    if display.refresh(fb, RefreshMode::M5Text).await.is_err() {
        error!("refresh failed");
    }
}

/// The sketch's `showImage`: reads the image from the card and shows it with `M5Quality`.
async fn show<B: EpdParallelBus, D: BlockDevice>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &mut ui::Fb<'_>,
    store: &mut ui::Store<D, FixedTime>,
    slots: &[Option<ui::ShortName>; 2],
    index: usize,
) {
    let wanted = ui::IMAGE_NAMES[index];
    let Some(name) = slots[index] else {
        status(display, fb, "PNG not found", wanted).await;
        info!("Image not found: {}", wanted);
        return;
    };
    // Like the sketch's `fillScreen(TFT_WHITE)` before `drawPngFile`: the screen goes white and stays
    // white while the file is read and decoded.
    let _ = fb.clear(Gray4::WHITE);
    if display.refresh(fb, RefreshMode::M5Quality).await.is_err() {
        error!("refresh failed");
    }
    let start = now_ms();
    let Ok(png) = store.read_file(name.as_str()) else {
        status(display, fb, "PNG load failed", wanted).await;
        error!("Failed to read {}", name.as_str());
        return;
    };
    let read = now_ms() - start;
    if draw_png(fb, &png).is_err() {
        status(display, fb, "PNG load failed", wanted).await;
        error!("Failed to draw PNG: {}", name.as_str());
        return;
    }
    drop(png);
    ui::draw_label(fb, BUILD);
    let decoded = now_ms() - start - read;
    let shown = now_ms();
    if display.refresh(fb, RefreshMode::M5Quality).await.is_err() {
        error!("refresh failed");
    }
    info!(
        "Displayed: {} ({}, read {} ms, decode and draw {} ms, refresh {} ms)",
        wanted,
        name.as_str(),
        read,
        decoded,
        now_ms() - shown
    );
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_m5paperdiy_embassy_examples::init!();
    rust_m5paperdiy_embassy_examples::start_rtos!(p);
    info!("sd_images: Rust, async");
    let mut i2c = rust_m5paperdiy_embassy_examples::pmic_i2c!(p);
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
        rust_m5paperdiy_embassy_examples::halt();
    };
    let cs = Output::new(p.GPIO47, Level::High, OutputConfig::default());
    // Infallible for an `Output` chip select.
    let device = ExclusiveDevice::new_no_delay(spi, cs).unwrap_or_else(|e| match e {});
    let mut store = ui::Store::new(SdCard::new(device, Delay::new()), FixedTime);

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
    // `setRotation(1)` of the sketch: landscape.
    fb.set_rotation(Rotation::Deg0);
    if display.clear().await.is_err() {
        error!("initial clear failed");
    }

    status(
        &mut display,
        &mut fb,
        "PaperDIY microSD",
        "Initializing card",
    )
    .await;

    let Some(bytes) = store.card_bytes() else {
        error!("microSD initialization failed");
        status(
            &mut display,
            &mut fb,
            "microSD mount failed",
            "Check card / FAT32",
        )
        .await;
        loop {
            Timer::after(Duration::from_secs(1)).await;
        }
    };
    info!("Card size: {} MB", bytes / (1024 * 1024));

    match store.append_log() {
        Ok(()) => info!("Wrote {}", ui::LOG_NAME),
        Err(_) => error!("Failed to open {} for writing", ui::LOG_NAME),
    }
    if let Ok(text) = store.read_log() {
        info!("Contents of {}:", ui::LOG_NAME);
        for line in text.split(|&b| b == b'\n') {
            if let Ok(line) = core::str::from_utf8(line)
                && !line.trim().is_empty()
            {
                info!("{}", line.trim_end());
            }
        }
    }

    let slots = match store.find_images() {
        Ok(slots) => slots,
        Err(_) => {
            error!("microSD initialization failed");
            status(
                &mut display,
                &mut fb,
                "microSD mount failed",
                "Check card / FAT32",
            )
            .await;
            loop {
                Timer::after(Duration::from_secs(1)).await;
            }
        }
    };

    let mut pager = ui::Pager::new(ui::IMAGE_NAMES.len());
    show(&mut display, &mut fb, &mut store, &slots, pager.current()).await;

    let (mut a_down, mut b_down) = (btn_a.is_low(), btn_b.is_low());
    loop {
        let (a, b) = (btn_a.is_low(), btn_b.is_low());
        if a && !a_down {
            show(&mut display, &mut fb, &mut store, &slots, pager.previous()).await;
        }
        if b && !b_down {
            show(&mut display, &mut fb, &mut store, &slots, pager.next()).await;
        }
        (a_down, b_down) = (a, b);
        Timer::after(Duration::from_millis(20)).await;
    }
}
