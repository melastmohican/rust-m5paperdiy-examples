//! Tests of `common/src/image.rs`: the size of a PNG or JPEG from its header, the decode to 8-bit
//! gray, and the scaled draw into a 16-level target. The pictures in `fixtures/images/` are all 40
//! by 20: the left half is black (or red) and the right half is white (or blue).

use embedded_graphics::{Pixel, pixelcolor::Gray4, prelude::*};
use m5paperdiy_common::image::{Gray, decode, dimensions, draw};

fn file(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/images/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A target in memory, one level 0 to 15 per pixel, that starts at 9 so a pixel the draw did not
/// touch can be told from black and white.
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![9; (width * height) as usize],
        }
    }

    fn at(&self, x: u32, y: u32) -> u8 {
        self.pixels[(y * self.width + x) as usize]
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
        for Pixel(p, color) in pixels {
            if p.x >= 0 && p.y >= 0 && (p.x as u32) < self.width && (p.y as u32) < self.height {
                self.pixels[(p.y as u32 * self.width + p.x as u32) as usize] = color.luma();
            }
        }
        Ok(())
    }
}

fn decoded(name: &str) -> Gray {
    decode(&file(name)).unwrap_or_else(|| panic!("{name} should decode"))
}

fn left_right(image: &Gray) -> (u8, u8) {
    let row = 10 * image.width as usize;
    (image.pixels[row + 5], image.pixels[row + 35])
}

#[test]
fn the_size_comes_from_the_header_of_a_png_or_a_jpeg() {
    for name in [
        "gray.png",
        "mid.png",
        "alpha.png",
        "rgb8.png",
        "palette.png",
        "gray.jpg",
        "color.jpg",
    ] {
        assert_eq!(dimensions(&file(name)), Some((40, 20)), "{name}");
    }
}

#[test]
fn the_size_of_something_that_is_not_a_picture_is_none() {
    assert_eq!(dimensions(&[]), None);
    assert_eq!(dimensions(b"<html>not a picture</html>"), None);
    assert_eq!(dimensions(&[0x89, b'P', b'N', b'G']), None);
    assert_eq!(dimensions(&[0xFF, 0xD8]), None);
}

#[test]
fn a_one_bit_gray_png_decodes_to_black_and_white() {
    let image = decoded("gray.png");
    assert_eq!((image.width, image.height), (40, 20));
    assert_eq!(image.pixels.len(), 40 * 20);
    assert_eq!(left_right(&image), (0, 255));
}

#[test]
fn an_eight_bit_gray_png_keeps_its_level() {
    let image = decoded("mid.png");
    assert!(
        image.pixels.iter().all(|&v| v == 128),
        "{:?}",
        &image.pixels[..8]
    );
}

#[test]
fn colors_become_their_luma() {
    // 0.30 R + 0.59 G + 0.11 B: red is about 76, blue about 28.
    for name in ["rgb8.png", "palette.png"] {
        let (red, blue) = left_right(&decoded(name));
        assert!((74..=78).contains(&red), "{name} red {red}");
        assert!((26..=30).contains(&blue), "{name} blue {blue}");
    }
}

#[test]
fn transparent_pixels_are_white() {
    let (left, right) = left_right(&decoded("alpha.png"));
    assert_eq!((left, right), (0, 255));
}

#[test]
fn a_jpeg_decodes_to_gray() {
    let (left, right) = left_right(&decoded("gray.jpg"));
    assert!(left <= 8 && right >= 247, "{left} {right}");
    let (red, blue) = left_right(&decoded("color.jpg"));
    assert!((64..=90).contains(&red), "red {red}");
    assert!((16..=44).contains(&blue), "blue {blue}");
}

#[test]
fn a_broken_picture_does_not_decode() {
    assert!(decode(&file("truncated.png")).is_none());
    assert!(decode(&file("truncated.jpg")).is_none());
    assert!(decode(&[]).is_none());
    assert!(decode(b"GIF89a not supported").is_none());
}

#[test]
fn a_huge_picture_is_refused_before_it_is_decoded() {
    // The header of a 60000 x 60000 PNG: the decoder would need gigabytes.
    let mut png = file("gray.png");
    png[16..20].copy_from_slice(&60000u32.to_be_bytes());
    png[20..24].copy_from_slice(&60000u32.to_be_bytes());
    assert!(decode(&png).is_none());
}

#[test]
fn a_picture_is_drawn_at_its_place_and_nowhere_else() {
    let image = decoded("gray.png");
    let mut canvas = Canvas::new(60, 40);
    draw(&mut canvas, 5, 3, 40, 20, &image);
    for y in 0..40 {
        for x in 0..60 {
            let inside = (5..45).contains(&x) && (3..23).contains(&y);
            let value = canvas.at(x, y);
            if !inside {
                assert_eq!(value, 9, "outside at {x},{y}");
            } else if x < 25 {
                assert_eq!(value, 0, "black half at {x},{y}");
            } else {
                assert_eq!(value, 15, "white half at {x},{y}");
            }
        }
    }
}

#[test]
fn a_picture_is_scaled_down_to_the_size_asked_for() {
    let image = decoded("gray.png");
    let mut canvas = Canvas::new(30, 20);
    draw(&mut canvas, 2, 1, 20, 10, &image);
    assert_eq!((canvas.at(2, 1), canvas.at(11, 5)), (0, 0));
    assert_eq!((canvas.at(12, 1), canvas.at(21, 10)), (15, 15));
    assert_eq!(
        (
            canvas.at(1, 1),
            canvas.at(22, 1),
            canvas.at(2, 0),
            canvas.at(2, 11)
        ),
        (9, 9, 9, 9)
    );
}

#[test]
fn a_picture_is_scaled_up_too() {
    let image = decoded("gray.png");
    let mut canvas = Canvas::new(100, 50);
    draw(&mut canvas, 0, 0, 80, 40, &image);
    assert_eq!((canvas.at(39, 20), canvas.at(40, 20)), (0, 15));
    assert_eq!((canvas.at(79, 39), canvas.at(80, 39)), (15, 9));
}

#[test]
fn a_pixel_averages_the_pixels_it_covers() {
    // The whole picture in one pixel is the average of black and white: half way, in levels 7 or 8.
    let image = decoded("gray.png");
    let mut canvas = Canvas::new(4, 4);
    draw(&mut canvas, 0, 0, 1, 1, &image);
    assert!((7..=8).contains(&canvas.at(0, 0)), "{}", canvas.at(0, 0));
}

#[test]
fn mid_gray_is_dithered_between_two_levels() {
    // 128 is between level 7 (112) and 8 (128): an ordered dither mixes them, like M5GFX does.
    let image = decoded("mid.png");
    let mut canvas = Canvas::new(40, 20);
    draw(&mut canvas, 0, 0, 40, 20, &image);
    let levels: std::collections::BTreeSet<u8> = canvas.pixels.iter().copied().collect();
    assert_eq!(levels.into_iter().collect::<Vec<_>>(), vec![7, 8]);
    let mean = canvas.pixels.iter().map(|&v| u32::from(v)).sum::<u32>() as f32 / 800.0;
    assert!((7.0..=8.0).contains(&mean), "{mean}");
}

#[test]
fn drawing_partly_off_the_target_or_nothing_does_not_panic() {
    let image = decoded("gray.png");
    let mut canvas = Canvas::new(20, 10);
    draw(&mut canvas, -10, -5, 40, 20, &image);
    draw(&mut canvas, 15, 8, 40, 20, &image);
    draw(&mut canvas, 0, 0, 0, 0, &image);
    draw(&mut canvas, 0, 0, 5, 0, &image);
    draw(
        &mut canvas,
        0,
        0,
        40,
        20,
        &Gray {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        },
    );
    assert_eq!(canvas.at(0, 0), 0);
}
