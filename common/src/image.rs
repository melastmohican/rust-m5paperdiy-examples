//! The pictures of a book: the size of a PNG or a JPEG from its header, the decode to 8-bit gray, and
//! a scaled draw into a 16-level target. The Arduino reader hands the bytes to M5GFX's `drawPng` and
//! `drawJpg`; this is the same job with `minipng` and `zune-jpeg`, and the dither M5GFX uses to get
//! from 256 levels to the panel's 16.

use alloc::vec::Vec;

use embedded_graphics::{Pixel, pixelcolor::Gray4, prelude::*};
use minipng::{BitDepth, ColorType};
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

/// The most memory a PNG may need for its decode, and the most pixels a JPEG may have. A bigger
/// picture is left out: the board has 8 MB of PSRAM for the page, the buffers and the picture.
const MAX_PNG_BYTES: usize = 3 << 20;
const MAX_JPEG_PIXELS: u64 = 3_000_000;

/// A picture as one byte a pixel, 0 black to 255 white, row after row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gray {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&PNG_SIGNATURE)
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn be16(bytes: &[u8]) -> u32 {
    u32::from(u16::from_be_bytes([bytes[0], bytes[1]]))
}

/// The width and height of a PNG or JPEG, read from its header. `None` for anything else, and for a
/// size of zero.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let (width, height) = if is_png(bytes) {
        // The first chunk is IHDR: length, "IHDR", then width and height.
        if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
            return None;
        }
        (be32(&bytes[16..20]), be32(&bytes[20..24]))
    } else if is_jpeg(bytes) {
        jpeg_dimensions(bytes)?
    } else {
        return None;
    };
    (width > 0 && height > 0).then_some((width, height))
}

/// Walks the segments of a JPEG to the first start-of-frame marker, which holds the size.
fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    loop {
        // Every segment starts with 0xFF and a marker; extra 0xFF bytes may pad it.
        if *bytes.get(at)? != 0xFF {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        match marker {
            0xFF => at += 1,
            // Markers with no length: padding, TEM, RSTn, SOI, EOI.
            0x00 | 0x01 | 0xD0..=0xD9 => at += 2,
            // Start of scan: no frame header came before it.
            0xDA => return None,
            _ => {
                let length = be16(bytes.get(at + 2..at + 4)?) as usize;
                // SOF0 to SOF15 except DHT (C4), JPG (C8) and DAC (CC).
                if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                    let frame = bytes.get(at + 5..at + 9)?;
                    return Some((be16(&frame[2..4]), be16(&frame[0..2])));
                }
                if length < 2 {
                    return None;
                }
                at += 2 + length;
            }
        }
    }
}

/// Decodes a PNG or a JPEG to gray. Transparent pixels come out white, like the sketch's white page
/// under them. `None` when the bytes are not a picture this can read, are cut short, or are too
/// big for the memory.
pub fn decode(bytes: &[u8]) -> Option<Gray> {
    if is_png(bytes) {
        decode_png(bytes)
    } else if is_jpeg(bytes) {
        decode_jpeg(bytes)
    } else {
        None
    }
}

/// 8-bit luma of a color over white, with the weights M5GFX uses.
fn gray_over_white(r: u8, g: u8, b: u8, a: u8) -> u8 {
    let luma = (u32::from(r) * 77 + u32::from(g) * 151 + u32::from(b) * 28) >> 8;
    let a = u32::from(a);
    ((luma * a + 255 * (255 - a) + 127) / 255).min(255) as u8
}

/// Sample `index` of a row, scaled to 8 bits.
fn sample(row: &[u8], index: usize, depth: BitDepth) -> u8 {
    match depth {
        BitDepth::Eight => row[index],
        BitDepth::Sixteen => row[index * 2],
        bits => {
            let bits = bits as usize;
            let at = index * bits;
            let value = (row[at / 8] >> (8 - bits - at % 8)) & ((1 << bits) - 1);
            (u32::from(value) * 255 / ((1 << bits) - 1)) as u8
        }
    }
}

fn decode_png(bytes: &[u8]) -> Option<Gray> {
    let header = minipng::decode_png_header(bytes).ok()?;
    if header.required_bytes() > MAX_PNG_BYTES {
        return None;
    }
    let mut scratch = Vec::new();
    scratch.try_reserve_exact(header.required_bytes()).ok()?;
    scratch.resize(header.required_bytes(), 0u8);
    let image = minipng::decode_png(bytes, &mut scratch).ok()?;
    let (width, height) = (image.width() as usize, image.height() as usize);
    let (depth, color) = (image.bit_depth(), image.color_type());
    let per_row = image.bytes_per_row();
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(width * height).ok()?;
    for row in image.pixels().chunks_exact(per_row).take(height) {
        for x in 0..width {
            let value = match color {
                ColorType::Gray => sample(row, x, depth),
                ColorType::GrayAlpha => {
                    let g = sample(row, x * 2, depth);
                    gray_over_white(g, g, g, sample(row, x * 2 + 1, depth))
                }
                ColorType::Rgb => gray_over_white(
                    sample(row, x * 3, depth),
                    sample(row, x * 3 + 1, depth),
                    sample(row, x * 3 + 2, depth),
                    255,
                ),
                ColorType::Rgba => gray_over_white(
                    sample(row, x * 4, depth),
                    sample(row, x * 4 + 1, depth),
                    sample(row, x * 4 + 2, depth),
                    sample(row, x * 4 + 3, depth),
                ),
                ColorType::Indexed => {
                    let [r, g, b, a] = image.palette(sample_index(row, x, depth));
                    gray_over_white(r, g, b, a)
                }
            };
            pixels.push(value);
        }
    }
    Some(Gray {
        width: width as u32,
        height: height as u32,
        pixels,
    })
}

/// The raw palette index of pixel `x`, which `sample` would stretch to 8 bits.
fn sample_index(row: &[u8], x: usize, depth: BitDepth) -> u8 {
    match depth {
        BitDepth::Eight | BitDepth::Sixteen => row[x],
        bits => {
            let bits = bits as usize;
            let at = x * bits;
            (row[at / 8] >> (8 - bits - at % 8)) & ((1 << bits) - 1)
        }
    }
}

fn decode_jpeg(bytes: &[u8]) -> Option<Gray> {
    let (width, height) = dimensions(bytes)?;
    if u64::from(width) * u64::from(height) > MAX_JPEG_PIXELS {
        return None;
    }
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::Luma);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let pixels = decoder.decode().ok()?;
    (pixels.len() == (width * height) as usize).then_some(Gray {
        width,
        height,
        pixels,
    })
}

/// M5GFX's 4 by 4 ordered dither, which `writeImage` uses from 8 bits to the panel's 16 levels.
const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/// 8-bit gray to a panel level 0 to 15, dithered by where the pixel is on the screen.
pub fn quantize(value: u8, x: u32, y: u32) -> u8 {
    let bayer = i32::from(BAYER[((y & 3) * 4 + (x & 3)) as usize]);
    ((i32::from(value) + bayer - 8) >> 4).clamp(0, 15) as u8
}

/// Draws `image` scaled to `width` by `height` with its top left at `(x, y)` of `target`. A pixel of
/// the result is the average of the source pixels it covers. What falls outside the target is cut.
pub fn draw<D: DrawTarget<Color = Gray4>>(
    target: &mut D,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    image: &Gray,
) {
    if width == 0 || height == 0 || image.width == 0 || image.height == 0 {
        return;
    }
    if image.pixels.len() < image.width as usize * image.height as usize {
        return;
    }
    let size = target.bounding_box().size;
    // The part of the result that is on the target.
    let first_x = (-x).clamp(0, width as i32) as u32;
    let first_y = (-y).clamp(0, height as i32) as u32;
    let last_x = (size.width as i32 - x).clamp(0, width as i32) as u32;
    let last_y = (size.height as i32 - y).clamp(0, height as i32) as u32;
    let source = |dest: u32, from: u32, to: u32| {
        let start = (u64::from(dest) * u64::from(from) / u64::from(to)) as u32;
        let end = (u64::from(dest + 1) * u64::from(from) / u64::from(to)) as u32;
        (start, end.max(start + 1).min(from))
    };
    for dy in first_y..last_y {
        let (y0, y1) = source(dy, image.height, height);
        let row = (first_x..last_x).map(|dx| {
            let (x0, x1) = source(dx, image.width, width);
            let mut sum = 0u32;
            for sy in y0..y1 {
                let line = sy as usize * image.width as usize;
                sum += image.pixels[line + x0 as usize..line + x1 as usize]
                    .iter()
                    .map(|&v| u32::from(v))
                    .sum::<u32>();
            }
            let value = (sum / ((y1 - y0) * (x1 - x0))) as u8;
            let (px, py) = (x + dx as i32, y + dy as i32);
            let level = quantize(value, px as u32, py as u32);
            Pixel(Point::new(px, py), Gray4::new(level))
        });
        let _ = target.draw_iter(row);
    }
}
