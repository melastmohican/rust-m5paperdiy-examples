//! # user_demo: port of the Arduino `PaperDIYUserDemo` sketch (M5Stack PaperDIY)
//!
//! The M5Stack PaperDIY demo: a 540 by 960 poster with live widgets on top, refreshed in small
//! windows. It is ported in stages, see the `user_demo` item in the `epdpi` backlog.
//!
//! **Stages 1 to 3 (this file):** the background image, the BtnA and BtnB border feedback, the
//! battery voltage and the card box. The
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
//! - **Stage 3:** the card box (`NO TF CARD`, or the card type, size and `WRITE`/`READ` speeds)
//!   follows the card detect pin (GPIO48) every 500 ms. A click of BtnB (a release, with both
//!   buttons up) shows `TESTING...` and writes then reads back a 1 MiB file in 16 KiB chunks,
//!   one chunk per loop pass so the buttons keep working, then shows the speeds. The test file
//!   is deleted afterwards.
//! - Every 40th partial refresh is a full `M5Quality` one, to clear ghosting, as in the sketch.
//! - Not in this stage: Wi-Fi list and the power-off dialog.
//! - The artwork is M5Stack's, copied from the Arduino sketch (`assets/img_bg.png`).
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::OutputPin;
use embedded_hal::i2c::I2c;
use embedded_hal::spi::SpiDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::sdcard::{CardType, DummyCsPin};
use embedded_sdmmc::{SdCard, TimeSource, Timestamp};
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::{Instant, Rate};
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
    use embedded_sdmmc::{
        BlockDevice, Error, Mode, RawDirectory, RawFile, RawVolume, TimeSource, VolumeIdx,
        VolumeManager,
    };
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

    /// Every 40th partial refresh becomes a full `M5Quality` one to clear ghosting, like the
    /// sketch's `PARTIAL_REFRESH_LIMIT`.
    #[derive(Default)]
    pub struct Partials(u32);

    impl Partials {
        pub const LIMIT: u32 = 40;

        /// Counts one partial refresh. True when this one must be a full refresh instead.
        pub fn bump(&mut self) -> bool {
            self.0 += 1;
            if self.0 >= Self::LIMIT {
                self.0 = 0;
                true
            } else {
                false
            }
        }
    }

    /// Turns button releases into clicks, the way the sketch's HAL does: a click is acted on
    /// once both buttons are up.
    #[derive(Default)]
    pub struct Clicks {
        a_down: bool,
        b_down: bool,
        pending_a: bool,
        pending_b: bool,
    }

    impl Clicks {
        /// Feed the current button states. Returns `(a_clicked, b_clicked)`.
        pub fn update(&mut self, a: bool, b: bool) -> (bool, bool) {
            self.pending_a |= self.a_down && !a;
            self.pending_b |= self.b_down && !b;
            self.a_down = a;
            self.b_down = b;
            if a || b {
                return (false, false);
            }
            let clicks = (self.pending_a, self.pending_b);
            self.pending_a = false;
            self.pending_b = false;
            clicks
        }
    }

    /// The card info box on the poster.
    pub const CARD_X: i32 = 121;
    pub const CARD_Y: i32 = 730;
    pub const CARD_W: i32 = 418 - CARD_X;
    pub const CARD_H: i32 = 831 - CARD_Y;

    const TEST_SIZE: usize = 1024 * 1024;
    const CHUNK: usize = 16 * 1024;
    const POLL_MS: u64 = 500;

    /// What the card test writes: the sketch's `testPattern`.
    fn pattern(i: usize) -> u8 {
        (i * 31 + 17) as u8
    }

    /// A card that can be forgotten and asked what it is, on top of reading blocks.
    pub trait Card: BlockDevice {
        /// The kind of card once it answers, `None` when it does not.
        fn kind(&self) -> Option<&'static str>;
        /// Forget the card so the next access starts it again, after a removal.
        fn forget(&self);
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum Status {
        NoCard,
        Mounted,
        MountFailed,
    }

    #[derive(Clone, Copy)]
    pub struct Info {
        pub status: Status,
        pub kind: &'static str,
        pub bytes: u64,
    }

    /// The outcome of a speed test.
    #[derive(Clone, Copy)]
    pub struct BenchResult {
        pub success: bool,
        pub write_mibs: f32,
        pub read_mibs: f32,
        pub message: &'static str,
    }

    impl BenchResult {
        pub const NONE: Self = Self {
            success: false,
            write_mibs: 0.0,
            read_mibs: 0.0,
            message: "",
        };

        fn failed(message: &'static str) -> Self {
            Self {
                message,
                ..Self::NONE
            }
        }
    }

    /// What the card box shows next.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum CardDraw {
        /// The card info and the last speeds.
        Info,
        /// `TESTING...` while the speed test runs.
        Testing,
    }

    /// MiB per second for `bytes` moved in `micros`.
    fn throughput(bytes: usize, micros: u64) -> f32 {
        if micros == 0 {
            return 0.0;
        }
        bytes as f32 * 1_000_000.0 / (micros as f32 * 1024.0 * 1024.0)
    }

    /// The speed test, a file of 1 MiB written and read back in 16 KiB chunks, one chunk per
    /// call to [`step`](Self::step) so the loop stays alive. Only the card calls are timed.
    struct Bench {
        buf: alloc::boxed::Box<[u8; CHUNK]>,
        volume: RawVolume,
        dir: RawDirectory,
        file: RawFile,
        name: Text,
        reading: bool,
        offset: usize,
        write_us: u64,
        read_us: u64,
    }

    impl Bench {
        fn start<D: Card, T: TimeSource, C: FnMut() -> u64>(
            mgr: &mut VolumeManager<D, T>,
            mut clock: C,
        ) -> Result<Self, &'static str> {
            let volume = mgr
                .open_raw_volume(VolumeIdx(0))
                .map_err(|_| "OPEN FAILED")?;
            let Ok(dir) = mgr.open_root_dir(volume) else {
                let _ = mgr.close_volume(volume);
                return Err("OPEN FAILED");
            };
            // A name that is free, so no user file is touched.
            let mut name = Text::new();
            let mut free = false;
            for attempt in 0..4u64 {
                let mut candidate = Text::new();
                let suffix = (clock() / 1000 + attempt) & 0xF_FFFF;
                let _ = write!(candidate, "PDT{suffix:05X}.BIN");
                if matches!(
                    mgr.find_directory_entry(dir, candidate.as_str()),
                    Err(Error::NotFound)
                ) {
                    name = candidate;
                    free = true;
                    break;
                }
            }
            let file = if free {
                mgr.open_file_in_dir(dir, name.as_str(), Mode::ReadWriteCreate)
                    .ok()
            } else {
                None
            };
            let Some(file) = file else {
                let _ = mgr.close_dir(dir);
                let _ = mgr.close_volume(volume);
                return Err("OPEN FAILED");
            };
            let mut buf = alloc::boxed::Box::new([0u8; CHUNK]);
            for (i, byte) in buf.iter_mut().enumerate() {
                *byte = pattern(i);
            }
            Ok(Self {
                buf,
                volume,
                dir,
                file,
                name,
                reading: false,
                offset: 0,
                write_us: 0,
                read_us: 0,
            })
        }

        /// Closes everything and deletes the test file. Errors are ignored: it is the way out.
        fn finish<D: Card, T: TimeSource>(&mut self, mgr: &mut VolumeManager<D, T>, close: bool) {
            if close {
                let _ = mgr.close_file(self.file);
            }
            let _ = mgr.delete_file_in_dir(self.dir, self.name.as_str());
            let _ = mgr.close_dir(self.dir);
            let _ = mgr.close_volume(self.volume);
        }

        /// One chunk of work. `Some` when the test is over.
        fn step<D: Card, T: TimeSource, C: FnMut() -> u64>(
            &mut self,
            mgr: &mut VolumeManager<D, T>,
            mut clock: C,
        ) -> Option<BenchResult> {
            if !self.reading {
                let start = clock();
                if mgr.write(self.file, &self.buf[..]).is_err() {
                    self.finish(mgr, true);
                    return Some(BenchResult::failed("WRITE FAILED"));
                }
                self.write_us += clock() - start;
                self.offset += CHUNK;
                if self.offset < TEST_SIZE {
                    return None;
                }
                // Closing flushes the file, which counts as writing.
                let start = clock();
                if mgr.close_file(self.file).is_err() {
                    self.finish(mgr, false);
                    return Some(BenchResult::failed("WRITE FAILED"));
                }
                self.write_us += clock() - start;
                match mgr.open_file_in_dir(self.dir, self.name.as_str(), Mode::ReadOnly) {
                    Ok(file) => {
                        self.file = file;
                        self.reading = true;
                        self.offset = 0;
                        None
                    }
                    Err(_) => {
                        self.finish(mgr, false);
                        Some(BenchResult::failed("READ OPEN FAILED"))
                    }
                }
            } else {
                let start = clock();
                let read = mgr.read(self.file, &mut self.buf[..]);
                let verified = matches!(read, Ok(CHUNK))
                    && self.buf.iter().enumerate().all(|(i, &b)| b == pattern(i));
                self.read_us += clock() - start;
                if !verified {
                    self.finish(mgr, true);
                    return Some(BenchResult::failed("READ VERIFY FAILED"));
                }
                self.offset += CHUNK;
                if self.offset < TEST_SIZE {
                    return None;
                }
                self.finish(mgr, true);
                Some(BenchResult {
                    success: true,
                    write_mibs: throughput(TEST_SIZE, self.write_us),
                    read_mibs: throughput(TEST_SIZE, self.read_us),
                    message: "OK",
                })
            }
        }
    }

    /// The card widget, the `AppTfCard` of the sketch: shows the card, watches for it being
    /// removed or inserted every 500 ms, and runs the speed test when B is clicked.
    pub struct CardApp<D: Card, T: TimeSource> {
        mgr: VolumeManager<D, T>,
        info: Info,
        mounted: bool,
        last_poll_ms: u64,
        drawn: bool,
        last_status: Status,
        bench: Option<Bench>,
        result: BenchResult,
    }

    impl<D: Card, T: TimeSource> CardApp<D, T> {
        pub fn new(device: D, time: T) -> Self {
            Self {
                mgr: VolumeManager::new(device, time),
                info: Info {
                    status: Status::NoCard,
                    kind: "",
                    bytes: 0,
                },
                mounted: false,
                last_poll_ms: 0,
                drawn: false,
                last_status: Status::NoCard,
                bench: None,
                result: BenchResult::NONE,
            }
        }

        pub fn info(&self) -> &Info {
            &self.info
        }

        pub fn result(&self) -> &BenchResult {
            &self.result
        }

        /// Looks at the card once and says to draw it, for the first frame.
        pub fn begin(&mut self, now_ms: u64, det_low: bool) -> CardDraw {
            self.update_card(now_ms, det_low, true, false);
            CardDraw::Info
        }

        /// Call every few milliseconds. `det_low` is the card detect pin (low means inserted),
        /// `b_clicked` a click of B, `clock` microseconds for timing the test. Returns what to
        /// draw when the box changed.
        pub fn update<C: FnMut() -> u64>(
            &mut self,
            now_ms: u64,
            det_low: bool,
            b_clicked: bool,
            clock: C,
        ) -> Option<CardDraw> {
            if let Some(bench) = &mut self.bench {
                let done = bench.step(&mut self.mgr, clock)?;
                self.bench = None;
                self.result = done;
                return Some(CardDraw::Info);
            }
            if b_clicked {
                let changed = self.update_card(now_ms, det_low, false, true);
                if self.info.status != Status::Mounted {
                    return changed.then_some(CardDraw::Info);
                }
                return match Bench::start(&mut self.mgr, clock) {
                    Ok(bench) => {
                        self.bench = Some(bench);
                        Some(CardDraw::Testing)
                    }
                    Err(message) => {
                        self.result = BenchResult::failed(message);
                        Some(CardDraw::Info)
                    }
                };
            }
            if now_ms - self.last_poll_ms >= POLL_MS
                && self.update_card(now_ms, det_low, false, false)
            {
                return Some(CardDraw::Info);
            }
            None
        }

        /// Reads the card state. True when the box must be drawn.
        fn update_card(&mut self, now_ms: u64, det_low: bool, force: bool, retry: bool) -> bool {
            self.last_poll_ms = now_ms;
            let previous = self.info.status;
            self.refresh_info(det_low, retry);
            if self.info.status != previous {
                self.result = BenchResult::NONE;
            }
            if force || !self.drawn || self.info.status != self.last_status {
                self.last_status = self.info.status;
                self.drawn = true;
                return true;
            }
            false
        }

        fn refresh_info(&mut self, det_low: bool, retry: bool) {
            if !det_low {
                if self.mounted {
                    self.mgr.device().forget();
                    self.mounted = false;
                }
                self.info = Info {
                    status: Status::NoCard,
                    kind: "",
                    bytes: 0,
                };
                return;
            }
            if self.mounted || (self.info.status == Status::MountFailed && !retry) {
                return;
            }
            self.info = Info {
                status: Status::MountFailed,
                kind: "",
                bytes: 0,
            };
            self.mgr.device().forget();
            let Some(kind) = self.mgr.device().kind() else {
                return;
            };
            // The filesystem has to be there too, like `SD.begin`.
            match self.mgr.open_raw_volume(VolumeIdx(0)) {
                Ok(volume) => {
                    let _ = self.mgr.close_volume(volume);
                }
                Err(_) => return,
            }
            let Ok(blocks) = self.mgr.device().num_blocks() else {
                return;
            };
            self.info = Info {
                status: Status::Mounted,
                kind,
                bytes: u64::from(blocks.0) * 512,
            };
            self.mounted = true;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        fb: &mut Fb<'_>,
        font: &FontRenderer,
        args: core::fmt::Arguments,
        x: i32,
        y: i32,
        h: HorizontalAlignment,
        v: VerticalPosition,
    ) {
        let _ = font.render_aligned(
            args,
            Point::new(x, y),
            v,
            h,
            FontColor::Transparent(Gray4::new(0)),
            fb,
        );
    }

    /// Clears the card box and draws `what` in it: the sketch's `drawInfo` and `drawStatus`.
    pub fn draw_card(fb: &mut Fb<'_>, info: &Info, result: &BenchResult, what: CardDraw) {
        use HorizontalAlignment::{Center, Left, Right};
        use VerticalPosition::Center as Middle;
        fill(fb, CARD_X, CARD_Y, CARD_W, CARD_H, 15);
        let (mid_x, mid_y) = (CARD_X + CARD_W / 2, CARD_Y + CARD_H / 2);
        let medium = FontRenderer::new::<fonts::u8g2_font_helvR18_tf>();
        let small = FontRenderer::new::<fonts::u8g2_font_helvR12_tf>();
        let bold = FontRenderer::new::<fonts::u8g2_font_fub17_tf>();
        if what == CardDraw::Testing {
            text(
                fb,
                &medium,
                format_args!("TESTING..."),
                mid_x,
                mid_y,
                Center,
                Middle,
            );
            return;
        }
        match info.status {
            Status::NoCard => text(
                fb,
                &bold,
                format_args!("NO TF CARD"),
                mid_x,
                mid_y,
                Center,
                Middle,
            ),
            Status::MountFailed => text(
                fb,
                &bold,
                format_args!("TF INIT FAILED"),
                mid_x,
                mid_y,
                Center,
                Middle,
            ),
            Status::Mounted => {
                let left = CARD_X + 7;
                let right = CARD_X + CARD_W - 7;
                let content_w = right - left;
                let gap = 14;
                let column_w = (content_w - gap) / 2;
                let right_column = left + column_w + gap;
                let header_y = CARD_Y + 20;
                let divider_y = CARD_Y + 40;
                let label_y = CARD_Y + 55;
                let value_y = CARD_Y + 79;
                text(
                    fb,
                    &medium,
                    format_args!("{}", info.kind),
                    left + 13,
                    header_y,
                    Left,
                    Middle,
                );
                text(
                    fb,
                    &medium,
                    format_args!("{:.1} GB", info.bytes as f32 / (1024.0 * 1024.0 * 1024.0)),
                    right - 13,
                    header_y,
                    Right,
                    Middle,
                );
                fill(fb, left, divider_y, content_w, 1, 10);
                if !result.message.is_empty() && !result.success {
                    text(
                        fb,
                        &small,
                        format_args!("TEST FAILED"),
                        mid_x,
                        label_y,
                        Center,
                        Middle,
                    );
                    text(
                        fb,
                        &medium,
                        format_args!("{}", result.message),
                        mid_x,
                        value_y,
                        Center,
                        Middle,
                    );
                } else {
                    fill(
                        fb,
                        mid_x,
                        divider_y + 8,
                        1,
                        CARD_H - (divider_y - CARD_Y) - 15,
                        10,
                    );
                    let write_x = left + column_w / 2;
                    let read_x = right_column + column_w / 2;
                    text(
                        fb,
                        &small,
                        format_args!("WRITE"),
                        write_x,
                        label_y,
                        Center,
                        Middle,
                    );
                    text(
                        fb,
                        &small,
                        format_args!("READ"),
                        read_x,
                        label_y,
                        Center,
                        Middle,
                    );
                    if result.success {
                        text(
                            fb,
                            &medium,
                            format_args!("{:.2} MB/s", result.write_mibs),
                            write_x,
                            value_y,
                            Center,
                            Middle,
                        );
                        text(
                            fb,
                            &medium,
                            format_args!("{:.2} MB/s", result.read_mibs),
                            read_x,
                            value_y,
                            Center,
                            Middle,
                        );
                    } else {
                        text(
                            fb,
                            &medium,
                            format_args!("-- MB/s"),
                            write_x,
                            value_y,
                            Center,
                            Middle,
                        );
                        text(
                            fb,
                            &medium,
                            format_args!("-- MB/s"),
                            read_x,
                            value_y,
                            Center,
                            Middle,
                        );
                    }
                }
            }
        }
    }
}

/// No clock on the board, so test files get a fixed date.
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

impl<S: SpiDevice<u8>, C: OutputPin, D: DelayNs> ui::Card for SdCard<S, C, D> {
    fn kind(&self) -> Option<&'static str> {
        self.get_card_type().map(|kind| match kind {
            CardType::SD1 | CardType::SD2 => "SDSC",
            CardType::SDHC => "SDHC/SDXC",
        })
    }

    fn forget(&self) {
        self.mark_card_uninit();
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

fn now_us() -> u64 {
    Instant::now().duration_since_epoch().as_micros()
}

/// Refreshes what changed with `mode` and returns how long it took in milliseconds. Every 40th
/// one is a full `M5Quality` refresh instead, to clear ghosting, like the sketch.
fn show_region<B: EpdParallelBus>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &mut ui::Fb<'_>,
    mode: RefreshMode,
    partials: &mut ui::Partials,
) -> u64 {
    let start = now_ms();
    let result = if partials.bump() {
        info!("full refresh to clear ghosting");
        fb.clear_dirty();
        display.refresh(fb, RefreshMode::M5Quality)
    } else {
        display.refresh_dirty(fb, mode)
    };
    if result.is_err() {
        error!("refresh failed");
    }
    now_ms() - start
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

    // microSD on SPI2: CLK 39, MOSI 38, MISO 40, CS 47, 20 MHz. Card detect is GPIO48, low when
    // a card is in.
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
    let sd_device = ExclusiveDevice::new_no_delay(spi, cs).unwrap_or_else(|e| match e {});
    let mut card = ui::CardApp::new(SdCard::new(sd_device, DummyCsPin, Delay::new()), FixedTime);
    let card_detect = Input::new(p.GPIO48, config);

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

    // The card box, like the sketch's `AppTfCard::begin`.
    let first = card.begin(now_ms(), card_detect.is_low());
    ui::draw_card(&mut fb, card.info(), card.result(), first);

    if display.refresh(&fb, RefreshMode::M5Quality).is_err() {
        error!("background refresh failed");
    }
    // The first refresh showed everything, only later changes are dirty.
    fb.clear_dirty();
    info!("ready");

    let mut clicks = ui::Clicks::default();
    let mut partials = ui::Partials::default();
    let (mut a_down, mut b_down) = (false, false);
    loop {
        let (a, b) = (btn_a.is_low(), btn_b.is_low());
        let (_, b_clicked) = clicks.update(a, b);
        if a != a_down || b != b_down {
            if a != a_down {
                ui::draw_border(&mut fb, ui::BORDER_A_Y, a);
            }
            if b != b_down {
                ui::draw_border(&mut fb, ui::BORDER_B_Y, b);
            }
            let took = show_region(&mut display, &mut fb, RefreshMode::M5Fastest, &mut partials);
            info!(
                "A {}, B {}: border refresh {} ms",
                if a { "down" } else { "up" },
                if b { "down" } else { "up" },
                took
            );
            (a_down, b_down) = (a, b);
        }
        if let Some(draw) = card.update(now_ms(), card_detect.is_low(), b_clicked, now_us) {
            // The card box: info is a quality refresh, `TESTING...` a text one.
            ui::draw_card(&mut fb, card.info(), card.result(), draw);
            let mode = match draw {
                ui::CardDraw::Info => RefreshMode::M5Quality,
                ui::CardDraw::Testing => RefreshMode::M5Text,
            };
            let took = show_region(&mut display, &mut fb, mode, &mut partials);
            let result = card.result();
            info!(
                "card box: refresh {} ms, write {} KiB/s, read {} KiB/s",
                took,
                (result.write_mibs * 1024.0) as u32,
                (result.read_mibs * 1024.0) as u32
            );
        } else if let Some(text) = battery.poll(now_ms(), a || b, || battery_mv(&mut i2c)) {
            // A new reading, shown in its box only.
            ui::draw_battery(&mut fb, text.as_str());
            let took = show_region(&mut display, &mut fb, RefreshMode::M5Text, &mut partials);
            info!("battery {}: refresh {} ms", text.as_str(), took);
        }
        delay.delay_ms(5);
    }
}
