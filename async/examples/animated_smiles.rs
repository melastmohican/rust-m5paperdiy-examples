//! # animated_smiles: async port of the FastEPD `animated_smiles` sketch (M5Stack PaperDIY)
//!
//! Eight 100 x 100 smileys bounce around the panel, as in `~/Src/Arduino/M5PaperDIY/FastEPD/animated_smiles`
//! (upstream `bitbank2/FastEPD`, `examples/Arduino/animated_smiles`). Each frame draws every smiley
//! at its position, moves it 4 px and bounces it off the edges, then refreshes what changed. After
//! 250 frames the panel is cleared to white and the board powers the panel down. The blocking twin
//! is `animated_smiles` in `rust-m5paperdiy-examples`. Here `epdpi` runs in async mode on the
//! `esp-rtos` Embassy executor and a row waits for the DMA instead of spinning.
//!
//! The smiley is the sketch's own `smiley.h`: 240 bytes of G5 (CCITT group 4) data. `mod ui` has a
//! small G5 decoder, a port of FastEPD's `g5dec.inl`, checked on the host against the output of the
//! C decoder.
//!
//! ## Run
//!
//! ```text
//! cargo run --release --example animated_smiles
//! ```
//!
//! ## Expected
//!
//! - The log prints `animated_smiles: Rust, async`, then `frame N: refresh N ms` every 50
//!   frames and, after 250 frames, `250 frames in N ms, N ms per frame`.
//! - Landscape, 960 x 540, white. Eight smileys (black outline on a white square) move
//!   diagonally in 4 px steps. The squares are opaque and the screen is never cleared, like the
//!   sketch, so the bits of outline that touch the edge of a square are left behind: two long
//!   diagonal lines per smiley, and the panel slowly fills with them.
//! - After the last frame the panel is held for 3 s, flashes to white and stays blank, with
//!   faint ghosts of the smileys left, which a run of `FastMono` updates always leaves.
//! - Differences from the sketch: `RefreshMode::FastMono` runs 4 passes where `setPasses(3)` runs
//!   3, the panel is powered on and off for every frame where `partialUpdate(true)` keeps it on,
//!   and the start positions come from the hardware random number generator. The Arduino sketch
//!   runs on the PaperDIY with FastEPD `main` (`BB_PANEL_M5PAPERS3` and a PM1 power-on, see its
//!   README); the released FastEPD 2.2.0 has no panel definition that fits. It shows the same
//!   trails, but ends on a clean white panel, where the final clear here leaves faint ghosts.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use defmt::{error, info};
use embassy_time::{Duration, Instant, Timer};
use esp_hal::delay::Delay;
use esp_hal::rng::Rng;
use esp_println as _;
use rust_m5paperdiy_embassy_examples::{epdpi::prelude::*, pmic};

esp_bootloader_esp_idf::esp_app_desc!();

/// The hardware-free part: the smiley, its G5 decoder and the motion of the eight copies. Same
/// text in the async crate, and tested on the host by `host-tests`.
mod ui {
    use embedded_graphics::{pixelcolor::Gray4, prelude::*};

    /// The sketch's `smiley.h`: a G5 file, 8 byte header (marker, width, height, size) then the
    /// compressed lines.
    pub const SMILEY: [u8; 240] = [
        0xbf, 0xbb, 0x64, 0x00, 0x64, 0x00, 0xe8, 0x00, 0x32, 0xff, 0x3a, 0x82, 0x93, 0xa3, 0x3d,
        0x04, 0x0e, 0x10, 0x70, 0x83, 0x84, 0x72, 0x62, 0x83, 0x84, 0x72, 0x05, 0x43, 0xa3, 0x8d,
        0x38, 0xe1, 0x02, 0x06, 0x1d, 0x04, 0x1b, 0xa0, 0x83, 0x70, 0x82, 0x0d, 0xd2, 0x61, 0xd2,
        0x6e, 0x82, 0x0d, 0xd2, 0x6e, 0x93, 0xe9, 0xba, 0x46, 0x5a, 0x5b, 0xa4, 0x61, 0xf4, 0x20,
        0xdb, 0xa4, 0x9a, 0x6d, 0xe9, 0x6d, 0xe9, 0x5a, 0xb7, 0x55, 0xbb, 0xd5, 0xab, 0xd2, 0xf6,
        0xf5, 0xef, 0x55, 0xbb, 0xd5, 0xab, 0xd7, 0xef, 0x5e, 0xf5, 0xfb, 0xd7, 0xbf, 0xfe, 0xbf,
        0x7a, 0xdd, 0xfb, 0x5f, 0xfa, 0xff, 0xbd, 0x2f, 0xbe, 0xd3, 0xff, 0xaf, 0xb4, 0xff, 0xeb,
        0xd3, 0x4d, 0x3f, 0x11, 0x8a, 0xff, 0xff, 0xf9, 0x88, 0x23, 0x34, 0x5e, 0x9a, 0x72, 0x52,
        0xf6, 0xbe, 0xd6, 0xd7, 0xff, 0xd6, 0xfe, 0xb7, 0xfa, 0x7e, 0xda, 0xda, 0x5d, 0x26, 0xfe,
        0xb7, 0xfb, 0x09, 0xfb, 0x41, 0xaa, 0xde, 0xd6, 0xbb, 0x25, 0x4c, 0x22, 0x77, 0xf6, 0xd3,
        0x0d, 0x26, 0x12, 0xdc, 0x43, 0x69, 0x0d, 0x6d, 0x86, 0x12, 0x5b, 0xb6, 0x95, 0x6d, 0x86,
        0x12, 0x5b, 0xb0, 0xc2, 0x55, 0xb6, 0x0c, 0x10, 0x4b, 0x6c, 0x33, 0x8e, 0x10, 0x92, 0xdb,
        0x14, 0xb7, 0x6a, 0xad, 0x86, 0x12, 0x5b, 0x61, 0x2d, 0xb0, 0xd2, 0x56, 0xc1, 0x82, 0x4a,
        0xd8, 0x30, 0x49, 0x5b, 0x15, 0xda, 0x56, 0xd2, 0xb0, 0xc2, 0x56, 0xd2, 0x86, 0xd2, 0xb0,
        0xc2, 0x0a, 0xc3, 0x09, 0x58, 0x61, 0x28, 0x60, 0xc1, 0x05, 0x67, 0x20, 0x51, 0x43, 0x39,
        0x31, 0x41, 0x43, 0x10, 0xa1, 0x85, 0x0c, 0x28, 0x30, 0x53, 0xa8, 0x29, 0x32, 0xef, 0x00,
    ];

    /// The marker at the start of a G5 file, `BB_BITMAP_MARKER`, little endian.
    const MARKER: u16 = 0xBBBF;
    /// Longest line the decoder takes, in pixels. Keeps every run position inside an `i16`.
    pub const MAX_WIDTH: usize = 2048;
    /// Entries in each of the two run lists (current and reference line).
    const MAX_FLIPS: usize = 512;

    /// FastEPD's `code_table`: for the next 7 bits, the code and its length in bits. `1` to `3` are
    /// V(-n), `0x11` to `0x13` are V(+n), `0x20` horizontal, `0x30` pass, the rest is an error.
    const CODE_TABLE: [u8; 128] = [
        0x90, 0, 0x40, 0, 3, 7, 0x13, 7, //
        2, 6, 2, 6, 0x12, 6, 0x12, 6, //
        0x30, 4, 0x30, 4, 0x30, 4, 0x30, 4, //
        0x30, 4, 0x30, 4, 0x30, 4, 0x30, 4, //
        0x20, 3, 0x20, 3, 0x20, 3, 0x20, 3, //
        0x20, 3, 0x20, 3, 0x20, 3, 0x20, 3, //
        0x20, 3, 0x20, 3, 0x20, 3, 0x20, 3, //
        0x20, 3, 0x20, 3, 0x20, 3, 0x20, 3, //
        1, 3, 1, 3, 1, 3, 1, 3, //
        1, 3, 1, 3, 1, 3, 1, 3, //
        1, 3, 1, 3, 1, 3, 1, 3, //
        1, 3, 1, 3, 1, 3, 1, 3, //
        0x11, 3, 0x11, 3, 0x11, 3, 0x11, 3, //
        0x11, 3, 0x11, 3, 0x11, 3, 0x11, 3, //
        0x11, 3, 0x11, 3, 0x11, 3, 0x11, 3, //
        0x11, 3, 0x11, 3, 0x11, 3, 0x11, 3, //
    ];

    /// Why a G5 file could not be decoded.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum G5Error {
        /// Wrong marker, a size of zero, or a header that does not fit the data.
        BadHeader,
        /// The image is wider than [`MAX_WIDTH`] or does not fit the sprite buffer.
        TooBig,
        /// The compressed data ends before the last line.
        OutOfData,
        /// A code that is not valid, or a run list that overflows.
        Corrupt,
    }

    /// Line by line G5 (CCITT group 4) decoder, a port of FastEPD's `g5dec.inl`.
    ///
    /// Every line is a list of run boundaries ("flips") coded against the line above. The lists
    /// of two lines are kept, and the one for the current line becomes the reference for the next.
    pub struct G5Decoder<'a> {
        data: &'a [u8],
        width: i32,
        height: usize,
        y: usize,
        /// Position in `data`, in bits.
        bit: usize,
        /// Bits in a long horizontal code: the bit length of the width.
        hlen: u32,
        cur: [i16; MAX_FLIPS],
        reference: [i16; MAX_FLIPS],
    }

    impl<'a> G5Decoder<'a> {
        /// Reads the header of `file` and prepares to decode its first line.
        pub fn new(file: &'a [u8]) -> Result<Self, G5Error> {
            let word = |at: usize| -> Option<u16> {
                Some(u16::from_le_bytes([*file.get(at)?, *file.get(at + 1)?]))
            };
            let (Some(marker), Some(width), Some(height), Some(size)) =
                (word(0), word(2), word(4), word(6))
            else {
                return Err(G5Error::BadHeader);
            };
            if marker != MARKER || width == 0 || height == 0 || size == 0 {
                return Err(G5Error::BadHeader);
            }
            if usize::from(width) > MAX_WIDTH {
                return Err(G5Error::TooBig);
            }
            let Some(data) = file.get(8..8 + usize::from(size)) else {
                return Err(G5Error::BadHeader);
            };
            let mut decoder = Self {
                data,
                width: i32::from(width),
                height: usize::from(height),
                y: 0,
                bit: 0,
                hlen: u16::BITS - width.leading_zeros(),
                cur: [0; MAX_FLIPS],
                reference: [0; MAX_FLIPS],
            };
            decoder.begin();
            Ok(decoder)
        }

        /// Image width in pixels.
        pub fn width(&self) -> usize {
            self.width as usize
        }

        /// Image height in pixels.
        pub fn height(&self) -> usize {
            self.height
        }

        /// Bytes in one decoded line.
        pub fn row_bytes(&self) -> usize {
            self.width().div_ceil(8)
        }

        /// Both lists start as "the run ends at the right edge", so the first line is coded against
        /// a white line. The last two entries are a sentinel that stops the searches.
        fn begin(&mut self) {
            let end = self.width as i16;
            for flip in self.cur.iter_mut().chain(self.reference.iter_mut()) {
                *flip = end;
            }
            for list in [&mut self.cur, &mut self.reference] {
                list[MAX_FLIPS - 2] = 0x7fff;
                list[MAX_FLIPS - 1] = 0x7fff;
            }
            self.bit = 0;
            self.y = 0;
        }

        /// The next `n` bits (at most 16) as a number, zeros past the end of the data.
        fn peek(&self, n: u32) -> u32 {
            let first = self.bit / 8;
            let mut word = 0u32;
            for i in 0..4 {
                word = (word << 8) | u32::from(self.data.get(first + i).copied().unwrap_or(0));
            }
            let shift = 32 - (self.bit % 8) as u32 - n;
            (word >> shift) & ((1 << n) - 1)
        }

        fn reference_at(&self, i: usize) -> Result<i32, G5Error> {
            self.reference
                .get(i)
                .map(|&flip| i32::from(flip))
                .ok_or(G5Error::Corrupt)
        }

        fn push(&mut self, at: &mut usize, value: i32) -> Result<(), G5Error> {
            let slot = self.cur.get_mut(*at).ok_or(G5Error::Corrupt)?;
            *slot = i16::try_from(value).map_err(|_| G5Error::Corrupt)?;
            *at += 1;
            Ok(())
        }

        /// Moves `r` forward over the reference runs that end at or before `a0`.
        fn skip_reference(&self, r: &mut usize, a0: i32) -> Result<(), G5Error> {
            while a0 >= self.reference_at(*r)? {
                *r += 2;
            }
            Ok(())
        }

        /// Reads the codes of one line into `cur`.
        fn decode_flips(&mut self) -> Result<(), G5Error> {
            let xsize = self.width;
            let (mut c, mut r) = (0usize, 0usize);
            let mut a0: i32 = -1;
            while a0 < xsize {
                if self.peek(1) == 1 {
                    // V(0), the most frequent code, one bit.
                    a0 = self.reference_at(r)?;
                    r += 1;
                    self.bit += 1;
                    self.push(&mut c, a0)?;
                    continue;
                }
                let index = (self.peek(8) & 0xfe) as usize;
                let code = CODE_TABLE[index];
                self.bit += usize::from(CODE_TABLE[index + 1]);
                match code {
                    1..=3 => {
                        // V(-1) to V(-3)
                        a0 = self.reference_at(r)? - i32::from(code);
                        self.push(&mut c, a0)?;
                        if r == 0 {
                            r += 2;
                        }
                        r -= 1;
                        self.skip_reference(&mut r, a0)?;
                    }
                    0x11..=0x13 => {
                        // V(+1) to V(+3)
                        let b1 = self.reference_at(r)?;
                        r += 1;
                        a0 = b1 + i32::from(code & 7);
                        if b1 != xsize && a0 < xsize {
                            self.skip_reference(&mut r, a0)?;
                        }
                        a0 = a0.min(xsize);
                        self.push(&mut c, a0)?;
                    }
                    0x20 => {
                        // Horizontal: two runs coded by length, short (3 bits) or long (`hlen`).
                        let from = a0.max(0);
                        let prefix = self.peek(2);
                        self.bit += 2;
                        let run = |decoder: &mut Self, long: bool| -> u32 {
                            let bits = if long { decoder.hlen } else { 3 };
                            let value = decoder.peek(bits);
                            decoder.bit += bits as usize;
                            value
                        };
                        let (long_first, long_second) = match prefix {
                            0 => (false, false),
                            1 => (false, true),
                            2 => (true, false),
                            _ => (true, true),
                        };
                        let first = run(self, long_first);
                        let second = run(self, long_second);
                        a0 = from + first as i32;
                        self.push(&mut c, a0)?;
                        a0 += second as i32;
                        if a0 < xsize {
                            self.skip_reference(&mut r, a0)?;
                        }
                        self.push(&mut c, a0)?;
                    }
                    0x30 => {
                        // Pass: skip a pair of reference runs.
                        r += 1;
                        a0 = self.reference_at(r)?;
                        r += 1;
                    }
                    _ => return Err(G5Error::Corrupt),
                }
            }
            // End the line with two entries at the right edge.
            self.push(&mut c, xsize)?;
            self.push(&mut c, xsize)
        }

        /// Writes the black runs of `cur` into a white line of 1 bit pixels, 1 is white.
        fn draw_line(&self, out: &mut [u8]) {
            let width = self.width;
            out[..self.row_bytes()].fill(0xff);
            let mut k = 0;
            let mut x = 0;
            while x < width {
                let (Some(&start), Some(&end)) = (self.cur.get(k), self.cur.get(k + 1)) else {
                    break;
                };
                k += 2;
                x = i32::from(start);
                let run = i32::from(end) - x;
                if x >= width || run <= 0 {
                    break;
                }
                for px in x.max(0)..(x + run).min(width) {
                    out[(px >> 3) as usize] &= !(0x80 >> (px & 7));
                }
            }
        }

        /// Decodes the next line into `out` (at least [`row_bytes`](Self::row_bytes) long), MSB
        /// first, 1 is a white pixel. Errors if called after the last line.
        pub fn decode_line(&mut self, out: &mut [u8]) -> Result<(), G5Error> {
            if out.len() < self.row_bytes() || self.y >= self.height {
                return Err(G5Error::TooBig);
            }
            if self.bit / 8 >= self.data.len() {
                return Err(G5Error::OutOfData);
            }
            self.decode_flips()?;
            self.draw_line(out);
            core::mem::swap(&mut self.cur, &mut self.reference);
            self.y += 1;
            Ok(())
        }
    }

    /// Largest sprite, in bytes of 1 bit pixels, with rows padded to whole bytes.
    pub const SPRITE_BYTES: usize = 13 * 100;

    /// A decoded black and white image.
    pub struct Sprite {
        width: usize,
        height: usize,
        bits: [u8; SPRITE_BYTES],
    }

    impl Sprite {
        /// Decodes a whole G5 file.
        pub fn from_g5(file: &[u8]) -> Result<Self, G5Error> {
            let mut decoder = G5Decoder::new(file)?;
            let stride = decoder.row_bytes();
            if stride * decoder.height() > SPRITE_BYTES {
                return Err(G5Error::TooBig);
            }
            let mut bits = [0u8; SPRITE_BYTES];
            for row in bits.chunks_mut(stride).take(decoder.height()) {
                decoder.decode_line(row)?;
            }
            Ok(Self {
                width: decoder.width(),
                height: decoder.height(),
                bits,
            })
        }

        /// Width in pixels.
        #[allow(dead_code)] // for the host tests
        pub fn width(&self) -> usize {
            self.width
        }

        /// Height in pixels.
        #[allow(dead_code)] // for the host tests
        pub fn height(&self) -> usize {
            self.height
        }

        /// True for a 1 bit, which the sketch draws in the foreground colour, white.
        pub fn is_set(&self, x: usize, y: usize) -> bool {
            let stride = self.width.div_ceil(8);
            x < self.width
                && y < self.height
                && self.bits[y * stride + x / 8] & (0x80 >> (x % 8)) != 0
        }

        /// Draws the sprite with its top left corner at `(x, y)`, opaque: 1 bits white, 0 bits
        /// black, like `loadG5Image(smiley, x, y, BBEP_WHITE, BBEP_BLACK)`.
        pub fn draw<D: DrawTarget<Color = Gray4>>(&self, target: &mut D, x: i32, y: i32) {
            let pixels = (0..self.height).flat_map(|row| {
                (0..self.width).map(move |col| {
                    let color = if self.is_set(col, row) {
                        Gray4::WHITE
                    } else {
                        Gray4::BLACK
                    };
                    Pixel(Point::new(x + col as i32, y + row as i32), color)
                })
            });
            let _ = target.draw_iter(pixels);
        }
    }

    /// Smileys on the panel, `NUM_SMILEYS` in the sketch.
    pub const NUM_SMILEYS: usize = 8;
    /// Frames the sketch runs before it clears the panel and stops.
    pub const FRAMES: u32 = 250;
    /// Pixels a smiley moves per frame in each direction.
    pub const STEP: i32 = 4;
    /// The sketch keeps a smiley this far from the left and top edge, and its bounce test
    /// leaves 4 px to the right and bottom: `width - 104`, `height - 104`.
    pub const MARGIN: i32 = 4;
    /// The sketch's 100 px sprite plus [`MARGIN`], the number it subtracts from the panel size.
    pub const SPAN: i32 = 104;

    /// One smiley: top left corner and direction.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Smiley {
        pub x: i32,
        pub y: i32,
        pub dx: i32,
        pub dy: i32,
    }

    /// Arduino's `random(min, max)`: a number from `min` up to but not including `max`.
    fn random_in(rng: &mut impl FnMut() -> u32, min: i32, max: i32) -> i32 {
        if max <= min {
            return min;
        }
        min + (rng() % (max - min) as u32) as i32
    }

    /// The eight smileys.
    pub struct Swarm {
        smileys: [Smiley; NUM_SMILEYS],
        max_x: i32,
        max_y: i32,
    }

    impl Swarm {
        /// Random start positions and directions, like the sketch's `setup()`, for a panel of
        /// `width` by `height` pixels.
        pub fn new(width: i32, height: i32, mut rng: impl FnMut() -> u32) -> Self {
            let (max_x, max_y) = (width - SPAN, height - SPAN);
            let mut smileys = [Smiley {
                x: 0,
                y: 0,
                dx: 0,
                dy: 0,
            }; NUM_SMILEYS];
            for smiley in smileys.iter_mut() {
                smiley.x = random_in(&mut rng, MARGIN, max_x);
                smiley.y = random_in(&mut rng, MARGIN, max_y);
                smiley.dx = if random_in(&mut rng, 0, 2) != 0 {
                    -STEP
                } else {
                    STEP
                };
                smiley.dy = if random_in(&mut rng, 0, 2) != 0 {
                    -STEP
                } else {
                    STEP
                };
            }
            Self {
                smileys,
                max_x,
                max_y,
            }
        }

        /// The smileys, in drawing order.
        #[allow(dead_code)] // for the host tests
        pub fn smileys(&self) -> &[Smiley] {
            &self.smileys
        }

        /// One frame of the sketch's inner loop: each smiley is drawn where it is, then moved and
        /// bounced off the edges.
        pub fn draw_frame<D: DrawTarget<Color = Gray4>>(
            &mut self,
            sprite: &Sprite,
            target: &mut D,
        ) {
            for smiley in self.smileys.iter_mut() {
                sprite.draw(target, smiley.x, smiley.y);
                smiley.x += smiley.dx;
                if smiley.x < MARGIN || smiley.x > self.max_x {
                    smiley.dx = -smiley.dx;
                }
                smiley.y += smiley.dy;
                if smiley.y < MARGIN || smiley.y > self.max_y {
                    smiley.dy = -smiley.dy;
                }
            }
        }
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) -> ! {
    let p = rust_m5paperdiy_embassy_examples::init!();
    rust_m5paperdiy_embassy_examples::start_rtos!(p);
    let mut i2c = rust_m5paperdiy_embassy_examples::pmic_i2c!(p);
    let mut delay = Delay::new();
    let _ = pmic::epd_power_on(&mut i2c, &mut delay);
    info!("animated_smiles: Rust, async");

    let bus = rust_m5paperdiy_embassy_examples::lcd_bus!(p, 16);
    let mut prev = vec![0x88u8; Ed047tc1::FB_BYTES];
    let mut line = vec![0u8; Ed047tc1::SCANLINE_BYTES];
    let mut fb_mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let (Ok(mut display), Ok(mut fb)) = (
        EpdDisplayBuilder::<Ed047tc1>::new().build(bus, &mut prev, &mut line),
        EpdFrameBuffer::<Ed047tc1>::new(&mut fb_mem),
    ) else {
        error!("buffer allocation too small");
        rust_m5paperdiy_embassy_examples::halt();
    };
    let Ok(sprite) = ui::Sprite::from_g5(&ui::SMILEY) else {
        error!("the smiley does not decode");
        rust_m5paperdiy_embassy_examples::halt();
    };

    // `epaper.clearWhite()`: the panel and the buffer start white.
    let clear_start = Instant::now();
    if display.clear().await.is_err() {
        error!("clear failed");
    }
    info!("clear at start: {} ms", clear_start.elapsed().as_millis());
    fb.clear_dirty();

    let rng = Rng::new();
    let mut swarm = ui::Swarm::new(Ed047tc1::WIDTH as i32, Ed047tc1::HEIGHT as i32, || {
        rng.random()
    });

    let start = Instant::now();
    for frame in 0..ui::FRAMES {
        swarm.draw_frame(&sprite, &mut fb);
        let refresh = Instant::now();
        // `epaper.partialUpdate(true)`: refresh what was drawn since the last frame.
        if display
            .refresh_dirty(&mut fb, RefreshMode::FastMono)
            .await
            .is_err()
        {
            error!("refresh failed");
        }
        if frame % 50 == 0 {
            info!(
                "frame {}: refresh {} ms",
                frame,
                refresh.elapsed().as_millis()
            );
        }
    }
    let total = start.elapsed().as_millis();
    info!(
        "{} frames in {} ms, {} ms per frame",
        ui::FRAMES,
        total,
        total / u64::from(ui::FRAMES)
    );

    Timer::after(Duration::from_secs(3)).await;
    // `epaper.clearWhite(false)` and `epaper.deInit()`.
    let clear_start = Instant::now();
    if display.clear().await.is_err() {
        error!("final clear failed");
    }
    info!("final clear: {} ms", clear_start.elapsed().as_millis());
    let _ = pmic::epd_power_off(&mut i2c);
    rust_m5paperdiy_embassy_examples::halt();
}
