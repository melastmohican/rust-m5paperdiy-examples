//! # factory_test: async port of the Arduino `FactoryTest` sketch (M5Stack PaperDIY)
//!
//! A two button menu with four pages, like the sketch: the factory test checklist, Compare (time
//! each refresh mode), Gray Ramp and the Game of Life. BtnA (GPIO4) moves to the next item or
//! runs the action, BtnB (GPIO3) selects or leaves a page. The blocking twin is `factory_test` in
//! `rust-m5paperdiy-examples`. Here `epdpi` runs in async mode on the `esp-rtos` Embassy
//! executor. `ui.rs` holds the drawing and the Life rules and is the same file in both crates.
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
//! - Factory Test: Battery (3.0 to 4.4 V passes), Wi-Fi (strongest access point above -55 dBm),
//!   PSRAM, SD Card (card size), Button A and Button B turn to `[PASS]` when they pass or are
//!   pressed. The scan takes a couple of seconds before the page appears. B leaves, A reruns
//!   once both buttons passed.
//! - Compare: A picks a mode, B refreshes with it and shows the time, hold B to leave.
//! - Gray Ramp: 16 bands, A inverts, B leaves.
//! - Life Game: A pauses, a second A resumes with new cells, B leaves.
//! - Differences from the Arduino sketch: `u8g2-fonts` Free Universal Bold and Helvetica
//!   instead of FreeSans, so letters differ a little.
#![no_std]
#![no_main]

extern crate alloc;

mod ui {
    //! Drawing, Game of Life and button logic of `factory_test`.
    //!
    //! No hardware in here, and this module is the same text in the blocking and the async
    //! crate: compare the block from `mod ui {` to its closing brace. The crate root must
    //! `use <crate>::epdpi;` for the paths below.

    use core::fmt::Write;

    use embedded_graphics::{
        pixelcolor::Gray4,
        prelude::*,
        primitives::{PrimitiveStyle, Rectangle, RoundedRectangle},
    };
    use u8g2_fonts::{
        FontRenderer, fonts,
        types::{FontColor, HorizontalAlignment, VerticalPosition},
    };

    use crate::BUILD;
    use crate::epdpi::prelude::*;

    /// Portrait width and height in pixels, with `Rotation::Deg270`.
    pub const W: i32 = 540;
    pub const H: i32 = 960;

    pub type Fb<'a> = EpdFrameBuffer<'a, Ed047tc1>;

    const BLACK: Gray4 = Gray4::new(0);
    const WHITE: Gray4 = Gray4::new(15);

    /// `FreeSansBold18pt7b` stand-in.
    fn bold() -> FontRenderer {
        FontRenderer::new::<fonts::u8g2_font_fub25_tf>()
    }

    /// `FreeSans12pt7b` stand-in.
    fn regular() -> FontRenderer {
        FontRenderer::new::<fonts::u8g2_font_helvR18_tf>()
    }

    /// A short piece of text in a fixed buffer, for detail columns and labels.
    #[derive(Clone, Copy)]
    pub struct Detail {
        buf: [u8; 40],
        len: usize,
    }

    impl Detail {
        pub const fn new() -> Self {
            Self {
                buf: [0; 40],
                len: 0,
            }
        }

        /// Replaces the text. Text that does not fit is cut off.
        pub fn set(&mut self, args: core::fmt::Arguments) {
            self.len = 0;
            let _ = self.write_fmt(args);
        }

        pub fn as_str(&self) -> &str {
            core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
        }
    }

    impl Default for Detail {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Write for Detail {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let room = self.buf.len() - self.len;
            let take = s.len().min(room);
            self.buf[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
            self.len += take;
            if take < s.len() {
                Err(core::fmt::Error)
            } else {
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        fb: &mut Fb,
        font: &FontRenderer,
        args: core::fmt::Arguments,
        x: i32,
        y: i32,
        h: HorizontalAlignment,
        v: VerticalPosition,
        color: Gray4,
    ) {
        let _ = font.render_aligned(
            args,
            Point::new(x, y),
            v,
            h,
            FontColor::Transparent(color),
            fb,
        );
    }

    fn hline(fb: &mut Fb, y: i32) {
        let _ = Rectangle::new(Point::new(0, y), Size::new(W as u32, 1))
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(fb);
    }

    fn fill(fb: &mut Fb, rect: Rectangle, color: Gray4) {
        let _ = rect.into_styled(PrimitiveStyle::with_fill(color)).draw(fb);
    }

    fn outline(fb: &mut Fb, rect: Rectangle) {
        let _ = rect
            .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
            .draw(fb);
    }

    /// White page, the title top left and a line under it.
    pub fn header(fb: &mut Fb, title: &str) {
        fb.fill(0xF);
        text(
            fb,
            &bold(),
            format_args!("{title}"),
            20,
            16,
            HorizontalAlignment::Left,
            VerticalPosition::Top,
            BLACK,
        );
        hline(fb, 64);
    }

    /// A line over a hint on each side at the bottom, and in the middle which build this is
    /// (`BUILD` from `main.rs`), so a photo says whether it is the Rust port.
    pub fn footer(fb: &mut Fb, left: &str, right: &str) {
        hline(fb, H - 48);
        let font = regular();
        text(
            fb,
            &font,
            format_args!("{left}"),
            20,
            H - 24,
            HorizontalAlignment::Left,
            VerticalPosition::Center,
            BLACK,
        );
        text(
            fb,
            &FontRenderer::new::<fonts::u8g2_font_helvR14_tf>(),
            format_args!("{BUILD}"),
            // Not centered: the left hint of the Compare page reaches to x = 222.
            305,
            H - 24,
            HorizontalAlignment::Center,
            VerticalPosition::Center,
            BLACK,
        );
        text(
            fb,
            &font,
            format_args!("{right}"),
            W - 20,
            H - 24,
            HorizontalAlignment::Right,
            VerticalPosition::Center,
            BLACK,
        );
    }

    /// One line of the checklist.
    #[derive(Clone, Copy)]
    pub struct Check {
        pub name: &'static str,
        pub pass: bool,
        pub detail: Detail,
    }

    impl Check {
        pub const fn new(name: &'static str) -> Self {
            Self {
                name,
                pass: false,
                detail: Detail::new(),
            }
        }
    }

    pub fn draw_checklist(fb: &mut Fb, items: &[Check]) {
        let passed = items.iter().filter(|c| c.pass).count();
        header(fb, "Factory Test");
        let font = regular();
        text(
            fb,
            &font,
            format_args!("PASS {}/{}", passed, items.len()),
            W - 20,
            24,
            HorizontalAlignment::Right,
            VerticalPosition::Top,
            BLACK,
        );
        const ROW_H: i32 = 64;
        for (i, item) in items.iter().enumerate() {
            let y = 84 + i as i32 * ROW_H;
            let mid = y + ROW_H / 2;
            // Failing items are inverted, like `ReversePartColor()` in the original.
            let ink = if item.pass {
                BLACK
            } else {
                fill(
                    fb,
                    Rectangle::new(Point::new(0, y), Size::new(W as u32, (ROW_H - 4) as u32)),
                    BLACK,
                );
                WHITE
            };
            let mark = if item.pass { "[PASS] " } else { "" };
            text(
                fb,
                &font,
                format_args!("{mark}{}.{}", i + 1, item.name),
                20,
                mid,
                HorizontalAlignment::Left,
                VerticalPosition::Center,
                ink,
            );
            text(
                fb,
                &font,
                format_args!("{}", item.detail.as_str()),
                W - 20,
                mid,
                HorizontalAlignment::Right,
                VerticalPosition::Center,
                ink,
            );
        }
        footer(fb, "A: rerun", "B: back");
    }

    pub const MENU: [&str; 4] = ["Factory Test", "Compare", "Gray Ramp", "Life Game"];

    pub fn draw_menu(fb: &mut Fb, selected: usize) {
        header(fb, "PaperDIY Factory Test");
        const ROW_H: i32 = 110;
        let font = bold();
        for (i, name) in MENU.iter().enumerate() {
            let y = 100 + i as i32 * (ROW_H + 16);
            let rect = Rectangle::new(Point::new(20, y), Size::new((W - 40) as u32, ROW_H as u32));
            let shape = RoundedRectangle::with_equal_corners(rect, Size::new(12, 12));
            let ink = if i == selected {
                let _ = shape.into_styled(PrimitiveStyle::with_fill(BLACK)).draw(fb);
                WHITE
            } else {
                let _ = shape
                    .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
                    .draw(fb);
                BLACK
            };
            text(
                fb,
                &font,
                format_args!("{name}"),
                W / 2,
                y + ROW_H / 2,
                HorizontalAlignment::Center,
                VerticalPosition::Center,
                ink,
            );
        }
        footer(fb, "A: next", "B: open");
    }

    /// A refresh mode of the Compare page.
    pub struct Mode {
        pub mode: RefreshMode,
        pub name: &'static str,
        pub usage: &'static str,
    }

    pub const MODES: [Mode; 4] = [
        Mode {
            mode: RefreshMode::M5Quality,
            name: "quality",
            usage: "High quality images",
        },
        Mode {
            mode: RefreshMode::M5Text,
            name: "text",
            usage: "Text with white background",
        },
        Mode {
            mode: RefreshMode::M5Fast,
            name: "fast",
            usage: "Graphics, fast flipping",
        },
        Mode {
            mode: RefreshMode::M5Fastest,
            name: "fastest",
            usage: "Menus, anti-aliased text",
        },
    ];

    pub fn draw_compare(fb: &mut Fb, selected: usize, ms: &[u32; 4]) {
        header(fb, "Compare");
        let font = regular();
        const BOX_H: i32 = 120;
        let cell = (W - 40) / 16;
        for (i, mode) in MODES.iter().enumerate() {
            let y = 84 + i as i32 * (BOX_H + 20);
            for g in 0..16 {
                fill(
                    fb,
                    Rectangle::new(Point::new(20 + g * cell, y), Size::new(cell as u32, 40)),
                    Gray4::new(g as u8),
                );
            }
            outline(
                fb,
                Rectangle::new(Point::new(20, y), Size::new((W - 40) as u32, BOX_H as u32)),
            );
            if i == selected {
                outline(
                    fb,
                    Rectangle::new(
                        Point::new(22, y + 2),
                        Size::new((W - 44) as u32, (BOX_H - 4) as u32),
                    ),
                );
            }
            text(
                fb,
                &font,
                format_args!("{}: {}", mode.name, mode.usage),
                28,
                y + 52,
                HorizontalAlignment::Left,
                VerticalPosition::Top,
                BLACK,
            );
            if ms[i] != 0 {
                text(
                    fb,
                    &font,
                    format_args!("{} ms", ms[i]),
                    W - 28,
                    y + BOX_H - 8,
                    HorizontalAlignment::Right,
                    VerticalPosition::Bottom,
                    BLACK,
                );
            }
        }
        footer(fb, "A: next / B: refresh", "hold B: back");
    }

    pub fn draw_ramp(fb: &mut Fb, inverted: bool) {
        fb.fill(0xF);
        let band = H / 16;
        for i in 0..16 {
            let level = if inverted { 15 - i } else { i };
            fill(
                fb,
                Rectangle::new(Point::new(0, i * band), Size::new(W as u32, band as u32)),
                Gray4::new(level as u8),
            );
        }
        // The original draws this in red, which the driver turns into a dark gray.
        text(
            fb,
            &bold(),
            format_args!("{}", if inverted { "15 -> 0" } else { "0 -> 15" }),
            W / 2,
            H / 2,
            HorizontalAlignment::Center,
            VerticalPosition::Center,
            Gray4::new(5),
        );
    }

    /// Cell size, and the grid that fits under the header and above the footer.
    pub const CELL: i32 = 12;
    pub const COLS: usize = (W / CELL) as usize;
    pub const ROWS: usize = ((H - 112) / CELL) as usize;

    /// Conway's Game of Life on a torus, seeded about 25 % alive.
    pub struct Life {
        cur: [u8; COLS * ROWS],
        next: [u8; COLS * ROWS],
        pub generation: u32,
        rng: u32,
    }

    impl Life {
        pub fn new(seed: u32) -> Self {
            let mut life = Self {
                cur: [0; COLS * ROWS],
                next: [0; COLS * ROWS],
                generation: 0,
                rng: seed | 1,
            };
            life.reseed();
            life
        }

        fn random(&mut self) -> u32 {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 17;
            self.rng ^= self.rng << 5;
            self.rng
        }

        /// New random cells and generation 0.
        pub fn reseed(&mut self) {
            for i in 0..COLS * ROWS {
                self.cur[i] = u8::from(self.random() % 100 < 25);
            }
            self.generation = 0;
        }

        pub fn alive(&self, x: usize, y: usize) -> bool {
            self.cur[y * COLS + x] != 0
        }

        #[allow(dead_code)] // for the host tests, which build patterns
        pub fn set(&mut self, x: usize, y: usize, alive: bool) {
            self.cur[y * COLS + x] = u8::from(alive);
        }

        /// One generation.
        pub fn step(&mut self) {
            for y in 0..ROWS {
                for x in 0..COLS {
                    let mut neighbours = 0;
                    for dy in [ROWS - 1, 0, 1] {
                        for dx in [COLS - 1, 0, 1] {
                            if dx != 0 || dy != 0 {
                                neighbours += self.cur[((y + dy) % ROWS) * COLS + (x + dx) % COLS];
                            }
                        }
                    }
                    let alive = self.cur[y * COLS + x] != 0;
                    self.next[y * COLS + x] = u8::from(if alive {
                        neighbours == 2 || neighbours == 3
                    } else {
                        neighbours == 3
                    });
                }
            }
            self.cur = self.next;
            self.generation += 1;
        }

        pub fn draw(&self, fb: &mut Fb, paused: bool) {
            header(fb, "Life Game");
            text(
                fb,
                &regular(),
                format_args!("gen {}", self.generation),
                W - 20,
                24,
                HorizontalAlignment::Right,
                VerticalPosition::Top,
                BLACK,
            );
            const TOP: i32 = 72;
            for y in 0..ROWS {
                for x in 0..COLS {
                    if self.alive(x, y) {
                        fill(
                            fb,
                            Rectangle::new(
                                Point::new(x as i32 * CELL + 1, TOP + y as i32 * CELL + 1),
                                Size::new((CELL - 2) as u32, (CELL - 2) as u32),
                            ),
                            BLACK,
                        );
                    }
                }
            }
            footer(
                fb,
                if paused {
                    "A: resume / reseed"
                } else {
                    "A: pause"
                },
                "B: back",
            );
        }
    }

    /// What a button did since the last [`Button::update`].
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub struct Events {
        /// Went down.
        pub pressed: bool,
        /// Came up before the hold time, a short click.
        pub clicked: bool,
        /// Has been down for the hold time. Once per press.
        pub hold: bool,
    }

    /// Edge and hold detection for an active low button, polled every 20 ms or so.
    pub struct Button {
        down: bool,
        since_ms: u64,
        hold_sent: bool,
    }

    impl Button {
        /// How long a press must last to count as a hold, like M5Unified's default.
        pub const HOLD_MS: u64 = 500;

        pub const fn new() -> Self {
            Self {
                down: false,
                since_ms: 0,
                hold_sent: false,
            }
        }

        pub fn update(&mut self, is_down: bool, now_ms: u64) -> Events {
            let mut events = Events::default();
            match (self.down, is_down) {
                (false, true) => {
                    self.since_ms = now_ms;
                    self.hold_sent = false;
                    events.pressed = true;
                }
                (true, true) => {
                    if !self.hold_sent && now_ms.saturating_sub(self.since_ms) >= Self::HOLD_MS {
                        self.hold_sent = true;
                        events.hold = true;
                    }
                }
                (true, false) => events.clicked = !self.hold_sent,
                (false, false) => {}
            }
            self.down = is_down;
            events
        }
    }

    impl Default for Button {
        fn default() -> Self {
            Self::new()
        }
    }
}

use defmt::{error, info};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c;
use embedded_hal::spi::SpiDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::SdCard;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_println as _;
use esp_radio::wifi::{ControllerConfig, WifiController, scan::ScanConfig};
use rust_m5paperdiy_embassy_examples::esp_alloc::{HEAP, MemoryCapability};
use rust_m5paperdiy_embassy_examples::{epdpi, pmic};

use epdpi::prelude::*;
use ui::{
    Button, Check, Events, Fb, Life, MENU, MODES, draw_checklist, draw_compare, draw_menu,
    draw_ramp,
};

esp_bootloader_esp_idf::esp_app_desc!();

/// Shown in the footer of every page, so a photo says which build it is.
const BUILD: &str = "epdpi async";

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
    Instant::now().as_millis()
}

/// Refreshes the screen with `mode` and returns how long it took in milliseconds.
async fn show<B: EpdParallelBus>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &Fb<'_>,
    mode: RefreshMode,
) -> u32 {
    let start = now_ms();
    if display.refresh(fb, mode).await.is_err() {
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
    let card = SdCard::new(spi, delay);
    card.num_bytes().ok().map(|bytes| bytes / (1024 * 1024))
}

/// Scans for access points: passes when the strongest one is above -55 dBm, like the sketch.
async fn wifi_check(radio: &mut Option<WifiController<'static>>, item: &mut Check) {
    let Some(controller) = radio else {
        item.detail.set(format_args!("radio failed"));
        return;
    };
    let config = ScanConfig::default().with_max(20);
    match controller.scan_async(&config).await {
        Ok(aps) => match aps.iter().max_by_key(|ap| ap.signal_strength) {
            Some(ap) => {
                item.pass = ap.signal_strength > -55;
                item.detail.set(format_args!(
                    "{} ({} dB)",
                    ap.ssid.as_str(),
                    ap.signal_strength
                ));
            }
            None => item.detail.set(format_args!("no AP")),
        },
        Err(_) => item.detail.set(format_args!("scan failed")),
    }
}

async fn run_checklist<B: EpdParallelBus, I: I2c, S: SpiDevice<u8>>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &mut Fb<'_>,
    pad: &mut Pad,
    i2c: &mut I,
    sd: &mut S,
    radio: &mut Option<WifiController<'static>>,
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
        wifi_check(radio, &mut items[1]).await;
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
        show(display, fb, RefreshMode::M5Quality).await;
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
                show(display, fb, RefreshMode::M5Text).await;
            }
            Timer::after(Duration::from_millis(20)).await;
        }
    }
}

async fn run_compare<B: EpdParallelBus>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &mut Fb<'_>,
    pad: &mut Pad,
) {
    let mut ms = [0u32; 4];
    let mut selected = 0;
    draw_compare(fb, selected, &ms);
    show(display, fb, RefreshMode::M5Quality).await;
    loop {
        let (a, b) = pad.poll();
        if a.pressed {
            selected = (selected + 1) % MODES.len();
            draw_compare(fb, selected, &ms);
            show(display, fb, RefreshMode::M5Fastest).await;
        }
        if b.clicked {
            draw_compare(fb, selected, &ms);
            ms[selected] = show(display, fb, MODES[selected].mode).await;
            info!("{}: {} ms", MODES[selected].name, ms[selected]);
            draw_compare(fb, selected, &ms);
            show(display, fb, RefreshMode::M5Fastest).await;
        }
        if b.hold {
            return;
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}

async fn run_ramp<B: EpdParallelBus>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &mut Fb<'_>,
    pad: &mut Pad,
) {
    let mut inverted = false;
    loop {
        draw_ramp(fb, inverted);
        show(display, fb, RefreshMode::M5Quality).await;
        loop {
            let (a, b) = pad.poll();
            if a.pressed {
                inverted = !inverted;
                break;
            }
            if b.pressed {
                return;
            }
            Timer::after(Duration::from_millis(20)).await;
        }
    }
}

async fn run_life<B: EpdParallelBus>(
    display: &mut EpdDisplay<'_, B, Ed047tc1>,
    fb: &mut Fb<'_>,
    pad: &mut Pad,
) {
    let mut life = Life::new(Instant::now().as_micros() as u32);
    let mut paused = false;
    loop {
        life.draw(fb, paused);
        let took = show(display, fb, RefreshMode::M5Fast).await;
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
            Timer::after(Duration::from_millis(20)).await;
        }
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_m5paperdiy_embassy_examples::init!();
    // The radio wants internal RAM, the sizes the `esp-radio` docs give. These go on the
    // `esp-alloc` that `esp-radio` and `esp-rtos` use, hence `esp-alloc` 0.10 in this crate.
    rust_m5paperdiy_embassy_examples::esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    rust_m5paperdiy_embassy_examples::esp_alloc::heap_allocator!(size: 36 * 1024);
    rust_m5paperdiy_embassy_examples::start_rtos!(p);
    let mut radio = match esp_radio::wifi::new(p.WIFI, ControllerConfig::default()) {
        Ok((controller, _interfaces)) => Some(controller),
        Err(_) => {
            error!("wifi init failed");
            None
        }
    };
    let mut i2c = rust_m5paperdiy_embassy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);

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
        rust_m5paperdiy_embassy_examples::halt();
    };
    let cs = Output::new(p.GPIO47, Level::High, OutputConfig::default());
    // Infallible for an `Output` chip select.
    let mut sd = ExclusiveDevice::new_no_delay(spi, cs).unwrap_or_else(|e| match e {});

    let bus = rust_m5paperdiy_embassy_examples::lcd_bus!(p, 16);
    let mut prev = alloc::vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut line = alloc::vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = alloc::vec![0xFFu8; Ed047tc1::FB_BYTES];
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

    let mut selected = 0;
    draw_menu(&mut fb, selected);
    show(&mut display, &fb, RefreshMode::M5Quality).await;
    info!("ready");
    loop {
        let (a, b) = pad.poll();
        if a.pressed {
            selected = (selected + 1) % MENU.len();
            draw_menu(&mut fb, selected);
            show(&mut display, &fb, RefreshMode::M5Fastest).await;
        }
        if b.pressed {
            match selected {
                0 => {
                    run_checklist(
                        &mut display,
                        &mut fb,
                        &mut pad,
                        &mut i2c,
                        &mut sd,
                        &mut radio,
                    )
                    .await
                }
                1 => run_compare(&mut display, &mut fb, &mut pad).await,
                2 => run_ramp(&mut display, &mut fb, &mut pad).await,
                _ => run_life(&mut display, &mut fb, &mut pad).await,
            }
            draw_menu(&mut fb, selected);
            show(&mut display, &fb, RefreshMode::M5Quality).await;
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}
