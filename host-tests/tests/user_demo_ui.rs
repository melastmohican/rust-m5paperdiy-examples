//! Tests of the poster, the dither, the border feedback and the battery widget of `examples/user_demo.rs`.

use host_tests::epdpi::prelude::*;
use host_tests::user_demo::*;

fn dump(fb: &EpdFrameBuffer<'_, Ed047tc1>, name: &str) {
    let dir = std::env::var("UI_DUMP").unwrap_or_default();
    if dir.is_empty() {
        return;
    }
    let mut out = format!("P5\n540 960\n255\n").into_bytes();
    for y in 0..960u32 {
        for x in 0..540u32 {
            out.push(fb.logical_pixel(x, y).unwrap() * 17);
        }
    }
    std::fs::write(format!("{dir}/{name}.pgm"), out).unwrap();
}

#[test]
fn quantize_keeps_the_ends_and_stays_in_range() {
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(quantize(0, x, y), 0);
            assert_eq!(quantize(255, x, y), 15);
            for v in 0..=255u8 {
                assert!(quantize(v, x, y) <= 15);
            }
        }
    }
    // Monotonic in the value at every position.
    for y in 0..4 {
        for x in 0..4 {
            for v in 1..=255u8 {
                assert!(quantize(v, x, y) >= quantize(v - 1, x, y));
            }
        }
    }
    // A mid gray dithers between two neighbouring levels over a 4 by 4 tile.
    let tile: Vec<u8> = (0..16).map(|i| quantize(130, i % 4, i / 4)).collect();
    assert!(
        tile.iter().any(|&l| l == 8) && tile.iter().any(|&l| l == 7 || l == 9),
        "{tile:?}"
    );
}

#[test]
fn the_real_poster_decodes_and_draws() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../blocking/assets/img_bg.png"
    ))
    .unwrap();
    let header = minipng::decode_png_header(&bytes).unwrap();
    let mut scratch = vec![0u8; header.required_bytes()];
    let image = minipng::decode_png(&bytes, &mut scratch).unwrap();
    assert_eq!((image.width(), image.height()), (540, 960));
    let mut mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut fb = EpdFrameBuffer::<Ed047tc1>::new(&mut mem).unwrap();
    fb.set_rotation(Rotation::Deg270);
    draw_gray8(&mut fb, image.pixels(), image.width(), image.height());
    dump(&fb, "poster");
    fb.clear_dirty();

    // Pressing A touches only the A label area.
    draw_border(&mut fb, BORDER_A_Y, true);
    let dirty = fb.dirty().expect("dirty after drawing a border");
    let logical = |r: Rect| (r.x, r.y, r.w, r.h);
    assert_eq!(
        logical(dirty),
        logical(fb.rect_to_panel(Rect::new(
            BORDER_X as u32,
            BORDER_A_Y as u32,
            (BORDER_RIGHT - BORDER_X + 1) as u32,
            BORDER_H as u32
        )))
    );
    assert_eq!(
        fb.logical_pixel(BORDER_X as u32 + 20, BORDER_A_Y as u32),
        Some(0),
        "top edge black"
    );
    assert_eq!(
        fb.logical_pixel(BORDER_X as u32, BORDER_A_Y as u32 + 30),
        Some(0),
        "left edge black"
    );
    assert_eq!(
        fb.logical_pixel(BORDER_RIGHT as u32 - 1, BORDER_A_Y as u32 + 33),
        Some(0),
        "right edge black"
    );
    // The far right top corner of the box is rounded off: not black.
    assert_ne!(
        fb.logical_pixel(BORDER_RIGHT as u32, BORDER_A_Y as u32),
        Some(0)
    );
    draw_border(&mut fb, BORDER_B_Y, true);
    dump(&fb, "both_down");
    draw_border(&mut fb, BORDER_A_Y, false);
    draw_border(&mut fb, BORDER_B_Y, false);
    assert_eq!(
        fb.logical_pixel(BORDER_X as u32 + 20, BORDER_A_Y as u32),
        Some(15),
        "released border is white"
    );
}

/// Runs the widget on a 5 ms tick from `from` to `to` ms and collects what it asks to draw.
fn run<F: FnMut() -> Option<u16>>(
    b: &mut Battery,
    from: u64,
    to: u64,
    interaction: bool,
    mut read: F,
) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut t = from;
    while t <= to {
        if let Some(text) = b.poll(t, interaction, &mut read) {
            out.push((t, text.as_str().to_string()));
        }
        t += 5;
    }
    out
}

#[test]
fn battery_first_reading_is_the_rounded_average_after_ten_samples() {
    let mut b = Battery::new();
    let mut n = 0;
    // 4026, 4035, 4026, ... average 4030.5 -> (sum + 5) / 10 = 4031 -> "4.03V"
    let drawn = run(&mut b, 0, 1000, false, || {
        n += 1;
        Some(if n % 2 == 1 { 4026 } else { 4035 })
    });
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].1, "4.03V");
    // 10 samples 50 ms apart: the ninth gap ends at 450 ms, drawn on the next tick.
    assert!((450..=460).contains(&drawn[0].0), "drawn at {}", drawn[0].0);
    assert_eq!(n, 10);
}

#[test]
fn battery_without_a_reading_shows_dashes() {
    let mut b = Battery::new();
    let drawn = run(&mut b, 0, 1000, false, || None);
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].1, "--.--V");
    let mut b = Battery::new();
    let drawn = run(&mut b, 0, 1000, false, || Some(0));
    assert_eq!(drawn[0].1, "--.--V", "zero is not a reading");
}

#[test]
fn battery_samples_every_ten_seconds_and_only_redraws_on_a_change() {
    let mut b = Battery::new();
    let reads = std::cell::Cell::new(0u32);
    let mv = std::cell::Cell::new(4032u16);
    let read = || {
        reads.set(reads.get() + 1);
        Some(mv.get())
    };
    assert_eq!(run(&mut b, 0, 1000, false, read).len(), 1);
    assert_eq!(reads.get(), 10);
    // Nothing for nine seconds, then 10 more samples at 10 s after the last one.
    assert!(run(&mut b, 1005, 9000, false, read).is_empty());
    assert_eq!(reads.get(), 10);
    // Same voltage: sampled again, no redraw.
    assert!(run(&mut b, 9005, 11_000, false, read).is_empty());
    assert_eq!(reads.get(), 20, "second round sampled");
    // Changed voltage: redrawn.
    mv.set(4010);
    let drawn = run(&mut b, 11_005, 22_000, false, read);
    assert_eq!(drawn.len(), 1);
    assert_eq!(drawn[0].1, "4.01V");
}

#[test]
fn battery_holds_a_finished_reading_while_a_button_is_down() {
    let mut b = Battery::new();
    assert!(
        run(&mut b, 0, 1000, true, || Some(4032)).is_empty(),
        "held back"
    );
    let drawn = run(&mut b, 1005, 1100, false, || Some(4032));
    assert_eq!(drawn.len(), 1, "drawn once the buttons are up");
    assert_eq!(drawn[0].1, "4.03V");
}

#[test]
fn battery_text_lands_inside_its_box_on_the_poster() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../blocking/assets/img_bg.png"
    ))
    .unwrap();
    let header = minipng::decode_png_header(&bytes).unwrap();
    let mut scratch = vec![0u8; header.required_bytes()];
    let image = minipng::decode_png(&bytes, &mut scratch).unwrap();
    let mut mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut fb = EpdFrameBuffer::<Ed047tc1>::new(&mut mem).unwrap();
    fb.set_rotation(Rotation::Deg270);
    draw_gray8(&mut fb, image.pixels(), image.width(), image.height());
    fb.clear_dirty();
    let before: Vec<u8> = (0..960u32)
        .flat_map(|y| (0..540u32).map(move |x| (x, y)))
        .map(|(x, y)| fb.logical_pixel(x, y).unwrap())
        .collect();
    draw_battery(&mut fb, "4.03V");
    dump(&fb, "battery");
    // Only pixels inside the box changed, and the box holds ink.
    let (bx, by, bw, bh) = (
        BATTERY_X as u32,
        BATTERY_Y as u32,
        BATTERY_W as u32,
        BATTERY_H as u32,
    );
    let mut ink = 0;
    for y in 0..960u32 {
        for x in 0..540u32 {
            let now = fb.logical_pixel(x, y).unwrap();
            let inside = x >= bx && x < bx + bw && y >= by && y < by + bh;
            if !inside {
                assert_eq!(
                    now,
                    before[(y * 540 + x) as usize],
                    "({x}, {y}) outside the box changed"
                );
            } else if now == 0 {
                ink += 1;
            }
        }
    }
    assert!(ink > 100, "ink {ink}");
    // The dirty area is the box, which is what the refresh will cover.
    let dirty = fb.dirty().unwrap();
    assert_eq!(
        (dirty.w, dirty.h),
        (
            fb.rect_to_panel(Rect::new(bx, by, bw, bh)).w,
            fb.rect_to_panel(Rect::new(bx, by, bw, bh)).h
        )
    );
}

#[test]
fn the_build_label_stays_in_the_top_right_corner_above_the_title() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../blocking/assets/img_bg.png"
    ))
    .unwrap();
    let header = minipng::decode_png_header(&bytes).unwrap();
    let mut scratch = vec![0u8; header.required_bytes()];
    let image = minipng::decode_png(&bytes, &mut scratch).unwrap();
    let mut mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut fb = EpdFrameBuffer::<Ed047tc1>::new(&mut mem).unwrap();
    fb.set_rotation(Rotation::Deg270);
    draw_gray8(&mut fb, image.pixels(), image.width(), image.height());
    let before: Vec<u8> = (0..960u32)
        .flat_map(|y| (0..540u32).map(move |x| (x, y)))
        .map(|(x, y)| fb.logical_pixel(x, y).unwrap())
        .collect();
    draw_label(&mut fb, "Rust: epdpi blocking");
    if let Some(dir) = std::env::var("UI_DUMP").ok().filter(|d| !d.is_empty()) {
        let mut out = format!("P5\n540 960\n255\n").into_bytes();
        for y in 0..960u32 {
            for x in 0..540u32 {
                out.push(fb.logical_pixel(x, y).unwrap() * 17);
            }
        }
        std::fs::write(format!("{dir}/poster_label.pgm"), out).unwrap();
    }
    let (mut ink, mut x0, mut x1, mut y1) = (0, u32::MAX, 0, 0);
    for y in 0..960u32 {
        for x in 0..540u32 {
            let now = fb.logical_pixel(x, y).unwrap();
            if now != before[(y * 540 + x) as usize] {
                ink += 1;
                (x0, x1, y1) = (x0.min(x), x1.max(x), y1.max(y));
            }
        }
    }
    assert!(ink > 80, "the label has ink: {ink}");
    // The top 26 rows only, right of the middle, and not to the very edge.
    assert!(
        y1 < 26 && x0 > 300 && x1 < 540 - 8,
        "label box x {x0}..{x1}, bottom {y1}"
    );
}
