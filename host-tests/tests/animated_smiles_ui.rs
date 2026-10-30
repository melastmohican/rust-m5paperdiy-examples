//! Tests of the `mod ui` block of `examples/animated_smiles.rs`: the G5 decoder against the output
//! of FastEPD's C decoder, bad input, and the bouncing of the eight smileys.
//!
//! `fixtures/fastepd_frames.cpp` runs the sketch's loop on FastEPD's own drawing code and prints a
//! hash of the picture after 1, 10, 100 and 250 frames, which `the_frames_match_fastepd_running_the_sketch`
//! compares with the Rust port's.
//!
//! `fixtures/smiley_g5dec.txt` is what `g5dec.inl` from FastEPD makes of the sketch's `smiley.h`,
//! one character per pixel, `#` for a 1 bit. `fixtures/g5dec_dump.cpp` is the program that wrote it.

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use host_tests::animated_smiles::*;

const REFERENCE: &str = include_str!("fixtures/smiley_g5dec.txt");

/// 960 x 540 with one byte per pixel, the panel the example draws on.
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        let (width, height) = (960, 540);
        Self {
            width,
            height,
            pixels: vec![0xff; (width * height) as usize],
        }
    }

    fn at(&self, x: i32, y: i32) -> u8 {
        self.pixels[(y as u32 * self.width + x as u32) as usize]
    }
}

impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }
}

impl DrawTarget for Canvas {
    type Color = Gray4;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Gray4>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        for Pixel(point, color) in pixels {
            if point.x >= 0
                && point.y >= 0
                && (point.x as u32) < self.width
                && (point.y as u32) < self.height
            {
                self.pixels[(point.y as u32 * self.width + point.x as u32) as usize] = color.luma();
            }
        }
        Ok(())
    }
}

/// A small deterministic generator, so the tests do not depend on a crate.
fn xorshift(mut state: u32) -> impl FnMut() -> u32 {
    move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    }
}

/// A generator that returns the given numbers in order, then zeros.
fn scripted(numbers: &[u32]) -> impl FnMut() -> u32 + '_ {
    let mut next = numbers.iter();
    move || next.next().copied().unwrap_or(0)
}

#[test]
fn the_smiley_decodes_to_what_the_c_decoder_makes() {
    let sprite = Sprite::from_g5(&SMILEY).unwrap();
    assert_eq!((sprite.width(), sprite.height()), (100, 100));
    let rows: Vec<&str> = REFERENCE.lines().collect();
    assert_eq!(rows.len(), 100);
    for (y, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), 100, "row {y}");
        for (x, ch) in row.bytes().enumerate() {
            assert_eq!(sprite.is_set(x, y), ch == b'#', "pixel ({x}, {y})");
        }
    }
}

#[test]
fn the_header_gives_the_size() {
    let decoder = G5Decoder::new(&SMILEY).unwrap();
    assert_eq!(
        (decoder.width(), decoder.height(), decoder.row_bytes()),
        (100, 100, 13)
    );
}

#[test]
fn a_bad_header_is_refused() {
    let mut wrong_marker = SMILEY;
    wrong_marker[0] = 0;
    assert_eq!(
        Sprite::from_g5(&wrong_marker).err(),
        Some(G5Error::BadHeader)
    );

    let mut no_width = SMILEY;
    no_width[2] = 0;
    no_width[3] = 0;
    assert_eq!(Sprite::from_g5(&no_width).err(), Some(G5Error::BadHeader));

    let mut no_data = SMILEY;
    no_data[6] = 0;
    no_data[7] = 0;
    assert_eq!(Sprite::from_g5(&no_data).err(), Some(G5Error::BadHeader));

    assert_eq!(
        Sprite::from_g5(&SMILEY[..7]).err(),
        Some(G5Error::BadHeader)
    );
    assert_eq!(
        Sprite::from_g5(&SMILEY[..100]).err(),
        Some(G5Error::BadHeader)
    );
    assert_eq!(Sprite::from_g5(&[]).err(), Some(G5Error::BadHeader));
}

#[test]
fn an_image_too_big_for_the_sprite_is_refused() {
    let mut wide = SMILEY;
    wide[2..4].copy_from_slice(&(MAX_WIDTH as u16 + 1).to_le_bytes());
    assert_eq!(Sprite::from_g5(&wide).err(), Some(G5Error::TooBig));

    let mut tall = SMILEY;
    tall[4..6].copy_from_slice(&200u16.to_le_bytes());
    assert_eq!(Sprite::from_g5(&tall).err(), Some(G5Error::TooBig));
}

#[test]
fn data_that_ends_early_is_an_error() {
    // The header says 232 bytes of data, but only the first 20 are used.
    let mut short = SMILEY;
    short[6..8].copy_from_slice(&20u16.to_le_bytes());
    let err = Sprite::from_g5(&short).err();
    assert!(
        matches!(err, Some(G5Error::OutOfData | G5Error::Corrupt)),
        "{err:?}"
    );
}

#[test]
fn garbage_never_panics() {
    let mut random = xorshift(0x1234_5678);
    for _ in 0..3000 {
        let mut file = SMILEY;
        for byte in file[8..].iter_mut() {
            *byte = random() as u8;
        }
        // Either it decodes to something or it says why not; it must not panic or hang.
        let _ = Sprite::from_g5(&file);
    }
    // Damage one byte of the real data at a time as well.
    for at in 8..SMILEY.len() {
        for flip in [0x01u8, 0x80, 0xff] {
            let mut file = SMILEY;
            file[at] ^= flip;
            let _ = Sprite::from_g5(&file);
        }
    }
}

#[test]
fn smileys_start_inside_the_margins_and_move_diagonally() {
    for seed in 1..200u32 {
        let swarm = Swarm::new(960, 540, xorshift(seed.wrapping_mul(2_654_435_761) | 1));
        assert_eq!(swarm.smileys().len(), NUM_SMILEYS);
        for smiley in swarm.smileys() {
            assert!((MARGIN..960 - SPAN).contains(&smiley.x), "{smiley:?}");
            assert!((MARGIN..540 - SPAN).contains(&smiley.y), "{smiley:?}");
            assert_eq!(smiley.dx.abs(), STEP);
            assert_eq!(smiley.dy.abs(), STEP);
        }
    }
}

#[test]
fn smileys_never_leave_the_panel() {
    let sprite = Sprite::from_g5(&SMILEY).unwrap();
    for seed in 1..20u32 {
        let mut swarm = Swarm::new(960, 540, xorshift(seed.wrapping_mul(2_654_435_761) | 1));
        let mut canvas = Canvas::new();
        for frame in 0..2000 {
            swarm.draw_frame(&sprite, &mut canvas);
            for smiley in swarm.smileys() {
                assert!(
                    (0..=960 - 100).contains(&smiley.x),
                    "frame {frame}: {smiley:?}"
                );
                assert!(
                    (0..=540 - 100).contains(&smiley.y),
                    "frame {frame}: {smiley:?}"
                );
            }
        }
    }
}

#[test]
fn a_smiley_bounces_off_the_right_edge() {
    // Smiley 0: x = 4 + 851 % 852 = 855, direction right (0), y = 4 + 0, direction right (0).
    let mut swarm = Swarm::new(960, 540, scripted(&[851, 0, 0, 0]));
    let first = swarm.smileys()[0];
    assert_eq!((first.x, first.dx), (855, STEP));
    let sprite = Sprite::from_g5(&SMILEY).unwrap();
    let mut canvas = Canvas::new();
    swarm.draw_frame(&sprite, &mut canvas);
    let moved = swarm.smileys()[0];
    // 855 + 4 = 859 is past 856, so it turns around and the next frame goes back.
    assert_eq!((moved.x, moved.dx), (859, -STEP));
    swarm.draw_frame(&sprite, &mut canvas);
    assert_eq!(swarm.smileys()[0].x, 855);
}

#[test]
fn a_smiley_bounces_off_the_left_edge() {
    // Smiley 0 at x = 4 going left (direction draw 1): 4 - 4 = 0 is below the margin.
    let mut swarm = Swarm::new(960, 540, scripted(&[0, 0, 1, 0]));
    assert_eq!(swarm.smileys()[0].x, MARGIN);
    let sprite = Sprite::from_g5(&SMILEY).unwrap();
    swarm.draw_frame(&sprite, &mut Canvas::new());
    let moved = swarm.smileys()[0];
    assert_eq!((moved.x, moved.dx), (0, STEP));
}

#[test]
fn a_frame_draws_the_sprite_where_the_smiley_was_then_moves_it() {
    // Smiley 0 at (400, 300) moving right and down; the other seven at (4, 4), far away.
    let mut swarm = Swarm::new(960, 540, scripted(&[396, 296, 0, 0]));
    let sprite = Sprite::from_g5(&SMILEY).unwrap();
    let mut canvas = Canvas::new();
    canvas.pixels.fill(0x77);
    swarm.draw_frame(&sprite, &mut canvas);
    let rows: Vec<&str> = REFERENCE.lines().collect();
    for (row, line) in rows.iter().enumerate() {
        for (col, ch) in line.bytes().enumerate() {
            let want = if ch == b'#' { 15 } else { 0 };
            assert_eq!(
                canvas.at(400 + col as i32, 300 + row as i32),
                want,
                "({col}, {row})"
            );
        }
    }
    // Opaque: nothing outside the 100 x 100 square of smiley 0 changed near it.
    assert_eq!(canvas.at(399, 300), 0x77);
    assert_eq!(canvas.at(400, 299), 0x77);
    assert_eq!(canvas.at(500, 300), 0x77);
    // And it moved on by one step.
    let moved = swarm.smileys()[0];
    assert_eq!((moved.x, moved.y), (404, 304));
}

/// The 32 numbers `random()` gave the C run, in the order the sketch's `setup()` asked for them
/// (x, y, direction x, direction y for each of the eight smileys). `fixtures/fastepd_frames.cpp`.
const C_RANDOM: [u32; 32] = [
    1804289383, 846930886, 1681692777, 1714636915, 1957747793, 424238335, 719885386, 1649760492,
    596516649, 1189641421, 1025202362, 1350490027, 783368690, 1102520059, 2044897763, 1967513926,
    1365180540, 1540383426, 304089172, 1303455736, 35005211, 521595368, 294702567, 1726956429,
    336465782, 861021530, 278722862, 233665123, 2145174067, 468703135, 1101513929, 1801979802,
];

/// Frames drawn, black pixels, and FNV-1a 64 of one byte per pixel (255 white, 0 black).
const C_FRAMES: [(u32, usize, u64); 4] = [
    (1, 20530, 0x1c41_b901_2492_7fa5),
    (10, 31256, 0x8412_c327_2dea_2d23),
    (100, 98173, 0x4031_38fd_901a_fdda),
    (250, 118455, 0x0f96_9f16_f2f2_efa2),
];

#[test]
fn the_frames_match_fastepd_running_the_sketch() {
    let sprite = Sprite::from_g5(&SMILEY).unwrap();
    let mut swarm = Swarm::new(960, 540, scripted(&C_RANDOM));
    let mut canvas = Canvas::new();
    let mut frames = 0;
    for (until, black, hash) in C_FRAMES {
        while frames < until {
            swarm.draw_frame(&sprite, &mut canvas);
            frames += 1;
        }
        let mut fnv = 0xcbf2_9ce4_8422_2325u64;
        for &pixel in &canvas.pixels {
            fnv ^= if pixel != 0 { 255 } else { 0 };
            fnv = fnv.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let dark = canvas.pixels.iter().filter(|&&pixel| pixel == 0).count();
        assert_eq!(dark, black, "black pixels after {until} frames");
        assert_eq!(fnv, hash, "picture after {until} frames");
    }
}
