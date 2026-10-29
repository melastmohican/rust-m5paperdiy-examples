//! Drawing, Game of Life and button logic of `factory_test`.
//!
//! No hardware in here, and the file is byte for byte the same in the blocking and the async
//! crate (`diff` them). `main.rs` must `use <crate>::epdpi;` for the paths below.

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
