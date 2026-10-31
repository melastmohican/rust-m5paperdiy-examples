//! A PNG decoded a row at a time, straight into a smaller picture. `minipng` needs the whole
//! decode in memory (11.5 MB of scratch for a 1600 by 2400 RGB cover, more than the board has).
//! This inflates the image data with `miniz_oxide` through a 32 KB window, unfilters one row,
//! turns it into gray and adds it into the row of the result it belongs to, so memory is the
//! window, two rows, a few small tables and the result.
//!
//! Only for a result no bigger than the picture (each result pixel then covers whole source
//! pixels, in ranges that do not overlap). The result is the same as decoding whole and calling
//! [`crate::image::scale`]. Interlaced PNGs are refused by `minipng` already, and the chunk CRCs are
//! not checked, as with `minipng`.

use alloc::boxed::Box;
use alloc::vec::Vec;

use minipng::{ColorType, ImageHeader};
use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};

use crate::image::{Gray, pixel_gray, source_range};

const WINDOW: usize = 32 * 1024;

/// A vector of `len` zeros, `None` when the memory is not there.
fn zeros<T: Copy + Default>(len: usize) -> Option<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(len).ok()?;
    v.resize(len, T::default());
    Some(v)
}

/// Undoes PNG filter `filter` of `line` in place, given the row before it. `bpp` is the bytes
/// per whole pixel, at least 1. `None` for a filter type that does not exist.
fn unfilter(filter: u8, line: &mut [u8], prev: &[u8], bpp: usize) -> Option<()> {
    for i in 0..line.len() {
        let left = if i >= bpp { line[i - bpp] } else { 0 };
        let up = prev[i];
        let up_left = if i >= bpp { prev[i - bpp] } else { 0 };
        let predictor = match filter {
            0 => 0,
            1 => left,
            2 => up,
            3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
            4 => paeth(left, up, up_left),
            _ => return None,
        };
        line[i] = line[i].wrapping_add(predictor);
    }
    Some(())
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// The picture `bytes` (whose header is `header`) as `width` by `height` gray pixels, each the
/// average of the pixels it covers. `None` when the picture is damaged, uses a filter or chunk
/// that does not exist, or the memory is not there; `width` and `height` must be at least 1 and
/// at most the size of the picture.
pub(crate) fn decode_scaled(
    bytes: &[u8],
    header: &ImageHeader,
    width: u32,
    height: u32,
) -> Option<Gray> {
    let (w, h) = (header.width(), header.height());
    if width == 0 || height == 0 || width > w || height > h {
        return None;
    }
    let (color, depth) = (header.color_type(), header.bit_depth());
    let row_bytes = header.bytes_per_row();
    let channels = match color {
        ColorType::Gray | ColorType::Indexed => 1,
        ColorType::GrayAlpha => 2,
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
    };
    let bpp = (channels * depth as usize / 8).max(1);

    // Which result column each source column goes to, and how many it takes.
    let mut column_of: Vec<u32> = zeros(w as usize)?;
    let mut columns: Vec<u32> = zeros(width as usize)?;
    for dx in 0..width {
        let (x0, x1) = source_range(dx, w, width);
        columns[dx as usize] = x1 - x0;
        for sx in x0..x1 {
            column_of[sx as usize] = dx;
        }
    }

    let mut palette = [[0u8, 0, 0, 255]; 256];
    let mut window: Vec<u8> = zeros(WINDOW)?;
    let mut inflater = Box::new(DecompressorOxide::new());
    let mut win_pos = 0usize;
    // A line as it comes out of the inflater: the filter byte, then the row.
    let mut line: Vec<u8> = zeros(1 + row_bytes)?;
    let mut prev: Vec<u8> = zeros(row_bytes)?;
    let mut fill = 0usize;
    let mut rows_done = 0u32;
    // The result row being summed, and the source rows it takes.
    let mut sums: Vec<u32> = zeros(width as usize)?;
    let (mut dy, mut y_range) = (0u32, source_range(0, h, height));
    let mut pixels: Vec<u8> = Vec::new();
    pixels
        .try_reserve_exact(width as usize * height as usize)
        .ok()?;
    let mut finished = false;

    let mut at = 8usize; // after the signature
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().ok()?) as usize;
        let kind: [u8; 4] = bytes[at + 4..at + 8].try_into().ok()?;
        let data = bytes.get(at + 8..at + 8 + len)?;
        at += 12 + len;
        match &kind {
            b"IEND" => break,
            b"PLTE" if color == ColorType::Indexed => {
                if len > 256 * 3 || !len.is_multiple_of(3) {
                    return None;
                }
                for (i, rgb) in data.chunks_exact(3).enumerate() {
                    palette[i][..3].copy_from_slice(rgb);
                }
            }
            b"tRNS" if color == ColorType::Indexed => {
                for (i, alpha) in data.iter().take(256).enumerate() {
                    palette[i][3] = *alpha;
                }
            }
            b"IDAT" if !finished => {
                let mut input = data;
                loop {
                    let flags = inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER
                        | inflate_flags::TINFL_FLAG_HAS_MORE_INPUT;
                    let (status, used, out) =
                        decompress(&mut inflater, input, &mut window, win_pos, flags);
                    input = &input[used..];
                    let mut produced = &window[win_pos..win_pos + out];
                    win_pos = (win_pos + out) & (WINDOW - 1);

                    while !produced.is_empty() {
                        let take = (line.len() - fill).min(produced.len());
                        line[fill..fill + take].copy_from_slice(&produced[..take]);
                        fill += take;
                        produced = &produced[take..];
                        if fill < line.len() {
                            continue;
                        }
                        fill = 0;
                        if rows_done >= h {
                            // More data than the picture has rows: ignore the rest.
                            break;
                        }
                        let (filter, row) = line.split_first_mut()?;
                        unfilter(*filter, row, &prev, bpp)?;
                        for (sx, &dx) in column_of.iter().enumerate() {
                            let gray = pixel_gray(row, sx, color, depth, &|i| palette[i as usize]);
                            sums[dx as usize] += u32::from(gray);
                        }
                        prev.copy_from_slice(row);
                        rows_done += 1;
                        if rows_done == y_range.1 {
                            let rows = y_range.1 - y_range.0;
                            for (sum, &count) in sums.iter_mut().zip(&columns) {
                                pixels.push((*sum / (rows * count)) as u8);
                                *sum = 0;
                            }
                            dy += 1;
                            if dy < height {
                                y_range = source_range(dy, h, height);
                            }
                        }
                    }

                    match status {
                        TINFLStatus::Done => {
                            finished = true;
                            break;
                        }
                        TINFLStatus::NeedsMoreInput => break,
                        TINFLStatus::HasMoreOutput => {}
                        _ => return None,
                    }
                }
            }
            _ => {}
        }
    }
    (rows_done == h && dy == height).then_some(Gray {
        width,
        height,
        pixels,
    })
}
