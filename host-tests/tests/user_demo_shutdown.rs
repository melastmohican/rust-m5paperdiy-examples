//! Tests of the power-off flow of `examples/user_demo.rs`.

use host_tests::epdpi::prelude::*;
use host_tests::user_demo::ShutdownAction as Act;
use host_tests::user_demo::*;

/// Runs the flow on a 5 ms tick, holding both buttons from `from` to `to`, and records the
/// actions. The dialog "takes" 1000 ms to draw, like a real refresh.
fn hold(s: &mut Shutdown, from: u64, to: u64, both: bool) -> Vec<(u64, Act)> {
    let mut out = Vec::new();
    let mut t = from;
    while t <= to {
        match s.update(t, both, both) {
            Act::Nothing => {}
            Act::ShowDialog => {
                out.push((t, Act::ShowDialog));
                t += 1000; // drawing the dialog takes a second
                s.dialog_shown(t);
            }
            Act::PowerOff => {
                out.push((t, Act::PowerOff));
                return out; // the real loop never comes back from power off
            }
            other => out.push((t, other)),
        }
        t += 5;
    }
    out
}

#[test]
fn holding_both_for_two_seconds_shows_the_dialog_then_counts_down_to_power_off() {
    let mut s = Shutdown::new();
    let acts = hold(&mut s, 0, 9000, true);
    let kinds: Vec<Act> = acts.iter().map(|(_, a)| *a).collect();
    assert_eq!(
        kinds,
        vec![
            Act::ShowDialog,
            Act::Countdown(4),
            Act::Countdown(3),
            Act::Countdown(2),
            Act::Countdown(1),
            Act::PowerOff
        ]
    );
    // The dialog after 2 s; the shown dialog starts the 5 s, ending about 5 s after it was drawn.
    assert!(
        (2000..=2010).contains(&acts[0].0),
        "dialog at {}",
        acts[0].0
    );
    let shown_at = acts[0].0 + 1000;
    assert!(
        (shown_at + 5000..=shown_at + 5010).contains(&acts.last().unwrap().0),
        "power off at {}",
        acts.last().unwrap().0
    );
    assert!(s.modal(), "the dialog is up until power off");
}

#[test]
fn letting_go_before_two_seconds_does_nothing() {
    let mut s = Shutdown::new();
    assert!(hold(&mut s, 0, 1990, true).is_empty());
    assert!(hold(&mut s, 1995, 3000, false).is_empty());
    assert!(!s.modal());
    // And the 2 s start over.
    let acts = hold(&mut s, 4000, 6100, true);
    assert_eq!(acts.len(), 1);
    assert!(acts[0].0 >= 6000);
}

#[test]
fn letting_go_during_the_countdown_cancels_and_waits_for_both_buttons() {
    let mut s = Shutdown::new();
    let acts = hold(&mut s, 0, 3500, true);
    assert_eq!(acts.first().map(|a| a.1), Some(Act::ShowDialog));
    assert!(s.modal());
    // One button up: cancelled.
    assert_eq!(s.update(3600, false, true), Act::Dismiss);
    assert!(!s.modal());
    // Still one button down: nothing, and holding both again does not restart yet.
    assert_eq!(s.update(3700, false, true), Act::Nothing);
    assert_eq!(s.update(3800, true, true), Act::Nothing);
    // Everything up, then it works again.
    assert_eq!(s.update(3900, false, false), Act::Nothing);
    assert_eq!(s.update(4000, true, true), Act::Nothing);
    let again = hold(&mut s, 4005, 6100, true);
    assert_eq!(again.first().map(|a| a.1), Some(Act::ShowDialog));
}

#[test]
fn clicks_of_a_suppressed_press_are_dropped_until_both_are_up() {
    let mut c = Clicks::default();
    c.update(true, true);
    c.suppress();
    assert_eq!(c.update(true, false), (false, false));
    assert_eq!(
        c.update(false, false),
        (false, false),
        "no click from the hold"
    );
    // Back to normal afterwards.
    c.update(false, true);
    assert_eq!(c.update(false, false), (false, true));
}

#[test]
fn saved_pixels_come_back_exactly_after_the_dialog() {
    let mut mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut fb = EpdFrameBuffer::<Ed047tc1>::new(&mut mem).unwrap();
    fb.set_rotation(Rotation::Deg270);
    // A pattern under the dialog.
    for y in 0..960u32 {
        for x in 0..540u32 {
            fb.set_logical_pixel(x, y, ((x * 3 + y * 7) % 16) as u8);
        }
    }
    let before: Vec<u8> = (0..960u32)
        .flat_map(|y| (0..540u32).map(move |x| (x, y)))
        .map(|(x, y)| fb.logical_pixel(x, y).unwrap())
        .collect();
    let saved = Saved::capture(&fb, DIALOG_X, DIALOG_Y, DIALOG_W, DIALOG_H);
    fb.clear_dirty();
    draw_dialog(&mut fb);
    let drawn: Vec<u8> = (0..960u32)
        .flat_map(|y| (0..540u32).map(move |x| (x, y)))
        .map(|(x, y)| fb.logical_pixel(x, y).unwrap())
        .collect();
    assert_ne!(drawn, before, "the dialog changed something");
    // Only inside the dialog.
    for y in 0..960i32 {
        for x in 0..540i32 {
            let inside = x >= DIALOG_X
                && x < DIALOG_X + DIALOG_W
                && y >= DIALOG_Y
                && y < DIALOG_Y + DIALOG_H;
            if !inside {
                assert_eq!(
                    drawn[(y * 540 + x) as usize],
                    before[(y * 540 + x) as usize],
                    "({x}, {y}) outside the dialog changed"
                );
            }
        }
    }
    let dirty = fb.dirty().unwrap();
    let want = fb.rect_to_panel(Rect::new(
        DIALOG_X as u32,
        DIALOG_Y as u32,
        DIALOG_W as u32,
        DIALOG_H as u32,
    ));
    assert!(
        dirty.w <= want.w + 2 && dirty.h <= want.h + 2,
        "dirty {dirty:?} vs {want:?}"
    );
    saved.restore(&mut fb);
    let after: Vec<u8> = (0..960u32)
        .flat_map(|y| (0..540u32).map(move |x| (x, y)))
        .map(|(x, y)| fb.logical_pixel(x, y).unwrap())
        .collect();
    assert_eq!(after, before, "restored exactly");
}

#[test]
fn the_countdown_line_stays_inside_the_dialog_and_the_dialog_renders() {
    let mut mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut fb = EpdFrameBuffer::<Ed047tc1>::new(&mut mem).unwrap();
    fb.set_rotation(Rotation::Deg270);
    draw_dialog(&mut fb);
    let dir = std::env::var("UI_DUMP").unwrap_or_default();
    let shot = |fb: &EpdFrameBuffer<'_, Ed047tc1>, name: &str| {
        if dir.is_empty() {
            return;
        }
        let mut out = Vec::new();
        for y in DIALOG_Y..DIALOG_Y + DIALOG_H {
            for x in DIALOG_X..DIALOG_X + DIALOG_W {
                out.push(fb.logical_pixel(x as u32, y as u32).unwrap() * 17);
            }
        }
        std::fs::write(format!("{dir}/{name}.raw"), out).unwrap();
    };
    shot(&fb, "dialog5");
    fb.clear_dirty();
    draw_countdown(&mut fb, 3);
    shot(&fb, "dialog3");
    let dirty = fb.dirty().unwrap();
    let line = fb.rect_to_panel(Rect::new(
        (DIALOG_X + 50) as u32,
        (DIALOG_Y + 119) as u32,
        (DIALOG_W - 100) as u32,
        54,
    ));
    assert_eq!((dirty.w, dirty.h), (line.w, line.h));
}
