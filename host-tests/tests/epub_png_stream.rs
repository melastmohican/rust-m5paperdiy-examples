//! Tests of the PNG decode a row at a time (`common/src/png_stream.rs`, reached through
//! `image::decode_scaled`): the result must be what decoding whole and scaling gives, for every
//! filter and color type, with the image data cut into several chunks, and a 1600 by 2400 picture
//! must decode in under 1 MB of memory. The PNGs are written here, filters included.

mod support;

use m5paperdiy_common::image::{decode, decode_scaled, scale};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------------------------
// An allocator that remembers the most memory in use, to measure what a decode needs.

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let live = LIVE.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
        PEAK.fetch_max(live, Ordering::SeqCst);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size >= layout.size() {
            let live = LIVE.fetch_add(new_size - layout.size(), Ordering::SeqCst)
                + (new_size - layout.size());
            PEAK.fetch_max(live, Ordering::SeqCst);
        } else {
            LIVE.fetch_sub(layout.size() - new_size, Ordering::SeqCst);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The most memory a call used above what was in use when it began.
fn peak_of<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let out = f();
    (out, PEAK.load(Ordering::SeqCst).saturating_sub(base))
}

// ---------------------------------------------------------------------------------------------
// A PNG writer: every row gets the next filter type, and the image data is cut into `pieces`.

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&support::crc32(&crc_input).to_be_bytes());
    out
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

/// `raw` is `height` rows of `row_bytes`. `color` is the PNG color type byte, `bpp` the bytes per pixel
/// for the filters (at least 1).
#[allow(clippy::too_many_arguments)]
fn png(
    width: u32,
    height: u32,
    color: u8,
    depth: u8,
    raw: &[u8],
    row_bytes: usize,
    bpp: usize,
    palette: Option<(&[u8], &[u8])>,
    pieces: usize,
    only_filter: Option<u8>,
) -> Vec<u8> {
    let mut filtered = Vec::new();
    for y in 0..height as usize {
        let row = &raw[y * row_bytes..(y + 1) * row_bytes];
        let prev = if y == 0 {
            vec![0; row_bytes]
        } else {
            raw[(y - 1) * row_bytes..y * row_bytes].to_vec()
        };
        let filter = only_filter.unwrap_or((y % 5) as u8);
        filtered.push(filter);
        for i in 0..row_bytes {
            let left = if i >= bpp { row[i - bpp] } else { 0 };
            let up = prev[i];
            let up_left = if i >= bpp { prev[i - bpp] } else { 0 };
            let predictor = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                _ => paeth(left, up, up_left),
            };
            filtered.push(row[i].wrapping_sub(predictor));
        }
    }
    let zlib = miniz_oxide::deflate::compress_to_vec_zlib(&filtered, 6);
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[depth, color, 0, 0, 0]);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    out.extend(chunk(b"IHDR", &header));
    if let Some((rgb, alpha)) = palette {
        out.extend(chunk(b"PLTE", rgb));
        if !alpha.is_empty() {
            out.extend(chunk(b"tRNS", alpha));
        }
    }
    let size = zlib.len().div_ceil(pieces.max(1));
    for part in zlib.chunks(size.max(1)) {
        out.extend(chunk(b"IDAT", part));
    }
    out.extend(chunk(b"IEND", &[]));
    out
}

/// Bytes that look like a picture: smooth runs with noise, so every filter does something.
fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed;
    let mut value = 128u8;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            value = value
                .wrapping_add(((state >> 24) % 9) as u8)
                .wrapping_sub(4);
            value
        })
        .collect()
}

fn same_as_whole(png: &[u8], width: u32, height: u32, what: &str) {
    let whole = decode(png).unwrap_or_else(|| panic!("{what}: the whole decode failed"));
    let expected = scale(&whole, width, height).unwrap();
    let streamed = decode_scaled(png, width, height)
        .unwrap_or_else(|| panic!("{what}: no result at {width} x {height}"));
    assert_eq!(streamed, expected, "{what} at {width} x {height}");
}

#[test]
fn every_color_type_and_filter_gives_what_the_whole_decode_scaled_gives() {
    let (w, h) = (53u32, 41u32);
    let cases: [(&str, u8, u8, usize); 8] = [
        ("gray 8", 0, 8, 1),
        ("gray 16", 0, 16, 2),
        ("gray alpha 8", 4, 8, 2),
        ("rgb 8", 2, 8, 3),
        ("rgb 16", 2, 16, 6),
        ("rgba 8", 6, 8, 4),
        ("gray 1", 0, 1, 0),
        ("gray 4", 0, 4, 0),
    ];
    for (name, color, depth, bytes_per_pixel) in cases {
        let (row_bytes, bpp) = if bytes_per_pixel == 0 {
            ((w as usize * depth as usize).div_ceil(8), 1)
        } else {
            (w as usize * bytes_per_pixel, bytes_per_pixel)
        };
        let raw = noise(
            row_bytes * h as usize,
            7 + u32::from(color) + u32::from(depth),
        );
        // Several pieces, so the data is cut mid-stream.
        let file = png(w, h, color, depth, &raw, row_bytes, bpp, None, 5, None);
        for (rw, rh) in [(53, 41), (26, 20), (17, 9), (5, 40), (1, 1), (53, 1)] {
            same_as_whole(&file, rw, rh, name);
        }
    }
}

#[test]
fn an_indexed_picture_with_transparency_is_the_same() {
    let (w, h) = (30u32, 22u32);
    for depth in [2u8, 4, 8] {
        let colors = 1usize << depth.min(8);
        let rgb: Vec<u8> = noise(colors * 3, 3);
        let alpha: Vec<u8> = (0..colors)
            .map(|i| if i % 3 == 0 { 0 } else { 255 })
            .collect();
        let row_bytes = (w as usize * depth as usize).div_ceil(8);
        let raw = noise(row_bytes * h as usize, 11);
        let file = png(
            w,
            h,
            3,
            depth,
            &raw,
            row_bytes,
            1,
            Some((&rgb, &alpha)),
            3,
            None,
        );
        same_as_whole(&file, 15, 11, &format!("indexed {depth}"));
        same_as_whole(&file, 30, 22, &format!("indexed {depth}"));
    }
}

#[test]
fn each_filter_alone_is_undone_right() {
    let (w, h) = (40u32, 30u32);
    let row_bytes = w as usize * 3;
    let raw = noise(row_bytes * h as usize, 99);
    for filter in 0..5u8 {
        let file = png(w, h, 2, 8, &raw, row_bytes, 3, None, 1, Some(filter));
        same_as_whole(&file, 20, 15, &format!("filter {filter}"));
    }
}

#[test]
fn the_pictures_of_the_tests_give_the_same_streamed_or_whole() {
    for name in [
        "gray.png",
        "mid.png",
        "palette.png",
        "rgb8.png",
        "alpha.png",
        "wide.png",
    ] {
        let path = format!(
            "{}/tests/fixtures/images/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let whole = decode(&file).unwrap();
        for (w, h) in [
            (whole.width, whole.height),
            (whole.width / 2, whole.height / 2),
            (7, 5),
        ] {
            same_as_whole(&file, w.max(1), h.max(1), name);
        }
    }
}

#[test]
fn a_result_bigger_than_the_picture_is_still_made() {
    // Not streamed, the whole decode is scaled up: small pictures only.
    let raw = noise(8 * 6, 5);
    let file = png(8, 6, 0, 8, &raw, 8, 1, None, 1, None);
    same_as_whole(&file, 16, 12, "scaled up");
    same_as_whole(&file, 16, 3, "wider, shorter");
}

#[test]
fn a_cover_of_1600_by_2400_decodes_in_under_a_megabyte() {
    let (w, h) = (1600u32, 2400u32);
    let row_bytes = w as usize * 3;
    let raw = noise(row_bytes * h as usize, 1);
    let file = png(w, h, 2, 8, &raw, row_bytes, 3, None, 40, None);
    drop(raw);
    let (streamed, used) = peak_of(|| decode_scaled(&file, 96, 145));
    let streamed = streamed.expect("the cover decodes");
    assert_eq!((streamed.width, streamed.height), (96, 145));
    println!("streamed: {used} bytes, input {} bytes", file.len());
    assert!(used < 1 << 20, "streamed decode used {used} bytes");
    // What the whole decode needs, for the record: more than the board has.
    let header = minipng::decode_png_header(&file).unwrap();
    assert!(header.required_bytes() > 8 << 20);
    // The whole decode is refused as too big, which is why the cover is streamed.
    assert!(decode(&file).is_none());
}

#[test]
fn damaged_pictures_do_not_panic_and_give_nothing() {
    let (w, h) = (20u32, 20u32);
    let raw = noise(w as usize * h as usize, 2);
    let file = png(w, h, 0, 8, &raw, w as usize, 1, None, 2, None);
    // Cut anywhere: never a panic, and a result only when the image data is all there.
    for cut in (0..file.len()).step_by(7) {
        let _ = decode_scaled(&file[..cut], 5, 5);
    }
    assert!(decode_scaled(&file[..file.len() / 2], 5, 5).is_none());
    // Bytes flipped anywhere: no panic.
    for at in (8..file.len()).step_by(5) {
        let mut broken = file.clone();
        broken[at] ^= 0xFF;
        let _ = decode_scaled(&broken, 5, 5);
    }
    // A filter type that does not exist.
    let mut bad = Vec::new();
    for y in 0..h as usize {
        bad.push(if y == 3 { 9 } else { 0 });
        bad.extend_from_slice(&raw[y * w as usize..(y + 1) * w as usize]);
    }
    let zlib = miniz_oxide::deflate::compress_to_vec_zlib(&bad, 6);
    let mut file = file[..33].to_vec();
    file.extend(chunk(b"IDAT", &zlib));
    file.extend(chunk(b"IEND", &[]));
    assert!(decode_scaled(&file, 5, 5).is_none());
    // Not a PNG at all.
    assert!(decode_scaled(b"hello", 5, 5).is_none());
    // A size of zero.
    assert!(decode_scaled(&png(4, 4, 0, 8, &[0; 16], 4, 1, None, 1, None), 0, 3).is_none());
}
