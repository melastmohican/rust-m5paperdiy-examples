//! Tests of the `mod ui` block of `examples/factory_test.rs`: Game of Life, buttons and pages.

use host_tests::epdpi::prelude::*;
use host_tests::factory_test::*;

fn fb_mem() -> Vec<u8> {
    vec![0xFFu8; Ed047tc1::FB_BYTES]
}

fn dump(fb: &EpdFrameBuffer<'_, Ed047tc1>, name: &str) {
    let dir = std::env::var("UI_DUMP").unwrap_or_default();
    if dir.is_empty() {
        return;
    }
    let mut out = format!("P5\n{} {}\n255\n", W, H).into_bytes();
    for y in 0..H as u32 {
        for x in 0..W as u32 {
            out.push(fb.logical_pixel(x, y).unwrap() * 17);
        }
    }
    std::fs::write(format!("{dir}/{name}.pgm"), out).unwrap();
}

fn portrait(mem: &mut Vec<u8>) -> EpdFrameBuffer<'_, Ed047tc1> {
    let mut fb = EpdFrameBuffer::<Ed047tc1>::new(mem).unwrap();
    fb.set_rotation(Rotation::Deg270);
    fb
}

#[test]
fn blinker_oscillates_with_period_two() {
    let mut life = Life::new(1);
    for y in 0..ROWS {
        for x in 0..COLS {
            life.set(x, y, false);
        }
    }
    for x in 10..13 {
        life.set(x, 10, true);
    }
    life.step();
    for y in 9..12 {
        assert!(life.alive(11, y), "vertical bar {y}");
    }
    assert!(!life.alive(10, 10) && !life.alive(12, 10));
    life.step();
    for x in 10..13 {
        assert!(life.alive(x, 10));
    }
    assert_eq!(life.generation, 2);
}

#[test]
fn block_is_still_and_the_grid_wraps() {
    let mut life = Life::new(1);
    for y in 0..ROWS {
        for x in 0..COLS {
            life.set(x, y, false);
        }
    }
    // A block straddling the corner: wraps on both axes.
    for (x, y) in [(COLS - 1, ROWS - 1), (0, ROWS - 1), (COLS - 1, 0), (0, 0)] {
        life.set(x, y, true);
    }
    life.step();
    for (x, y) in [(COLS - 1, ROWS - 1), (0, ROWS - 1), (COLS - 1, 0), (0, 0)] {
        assert!(life.alive(x, y));
    }
    let alive = (0..ROWS)
        .flat_map(|y| (0..COLS).map(move |x| (x, y)))
        .filter(|&(x, y)| life.alive(x, y))
        .count();
    assert_eq!(alive, 4);
}

#[test]
fn reseed_is_about_a_quarter_alive_and_resets_the_generation() {
    let mut life = Life::new(12345);
    life.step();
    life.reseed();
    assert_eq!(life.generation, 0);
    let alive = (0..ROWS)
        .flat_map(|y| (0..COLS).map(move |x| (x, y)))
        .filter(|&(x, y)| life.alive(x, y))
        .count();
    let share = alive as f32 / (ROWS * COLS) as f32;
    assert!((0.20..0.30).contains(&share), "share {share}");
}

#[test]
fn button_click_hold_and_bounce() {
    let mut b = Button::new();
    assert_eq!(b.update(false, 0), Events::default());
    let e = b.update(true, 100);
    assert!(e.pressed && !e.clicked && !e.hold);
    assert_eq!(b.update(true, 400), Events::default());
    let e = b.update(false, 450);
    assert!(e.clicked && !e.pressed);
    // A hold fires once, and the release afterwards is not a click.
    b.update(true, 1000);
    assert!(!b.update(true, 1400).hold);
    assert!(b.update(true, 1500).hold);
    assert!(!b.update(true, 1700).hold);
    assert!(!b.update(false, 1800).clicked);
}

#[test]
fn pages_draw_with_the_expected_landmarks() {
    let mut mem = fb_mem();
    let mut fb = portrait(&mut mem);

    draw_menu(&mut fb, 1);
    dump(&fb, "menu");
    assert_eq!(fb.logical_pixel(10, 64), Some(0), "header line");
    assert_eq!(fb.logical_pixel(10, H as u32 - 48), Some(0), "footer line");
    // Item 1 (Compare) is selected and filled, item 0 is only outlined.
    let item_y = |i: u32| 100 + i * 126 + 5;
    assert_eq!(fb.logical_pixel(30, item_y(1)), Some(0));
    assert_eq!(fb.logical_pixel(30, item_y(0)), Some(15));

    draw_ramp(&mut fb, false);
    dump(&fb, "ramp");
    assert_eq!(fb.logical_pixel(5, 5), Some(0));
    assert_eq!(fb.logical_pixel(5, 16 * 60 - 5), Some(15));
    draw_ramp(&mut fb, true);
    assert_eq!(fb.logical_pixel(5, 5), Some(15));
    assert_eq!(fb.logical_pixel(5, 16 * 60 - 5), Some(0));

    let mut ms = [0u32; 4];
    ms[2] = 304;
    draw_compare(&mut fb, 2, &ms);
    dump(&fb, "compare");
    // The gray strip of the third box: 16 cells from black to white.
    let y = 84 + 2 * 140 + 10;
    assert_eq!(fb.logical_pixel(25, y), Some(0));
    assert_eq!(fb.logical_pixel(20 + 15 * 31 + 5, y), Some(15));

    let mut items = [
        Check::new("Battery"),
        Check::new("PSRAM"),
        Check::new("SD Card"),
    ];
    items[0].pass = true;
    items[0].detail.set(format_args!("{:.2} V", 4.032));
    items[2].detail.set(format_args!("not ported yet"));
    draw_checklist(&mut fb, &items);
    dump(&fb, "checklist");
    // Row 0 passed (white), row 1 failed (inverted, black bar).
    assert_eq!(fb.logical_pixel(2, 84 + 5), Some(15));
    assert_eq!(fb.logical_pixel(2, 84 + 64 + 5), Some(0));

    let life = Life::new(7);
    draw_menu(&mut fb, 0);
    life.draw(&mut fb, false);
    dump(&fb, "life");
    life.draw(&mut fb, true);
}

#[test]
fn detail_cuts_long_text_instead_of_panicking() {
    let mut d = Detail::new();
    d.set(format_args!("{}", "x".repeat(100)));
    assert_eq!(d.as_str().len(), 40);
}
