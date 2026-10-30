//! Tests of the Wi-Fi list widget of `examples/user_demo.rs`.

use host_tests::epdpi::prelude::*;
use host_tests::user_demo::*;
use u8g2_fonts::{FontRenderer, fonts};

fn small() -> FontRenderer {
    FontRenderer::new::<fonts::u8g2_font_helvR12_tf>()
}

#[test]
fn one_scan_at_a_time() {
    let mut w = WifiApp::new();
    assert!(!w.scanning());
    assert!(w.start());
    assert!(w.scanning());
    assert!(!w.start(), "a second start is refused while one runs");
    w.finish(std::iter::empty());
    assert!(!w.scanning());
    assert!(w.start());
    w.fail();
    assert!(!w.scanning() && w.start());
}

#[test]
fn results_are_sorted_strongest_first_and_keep_everything() {
    let mut w = WifiApp::new();
    w.start();
    w.finish([("weak", -90), ("strong", -30), ("mid", -60), ("", -70)]);
    let names: Vec<(&str, i32)> = w
        .networks()
        .iter()
        .map(|n| (n.ssid.as_str(), n.rssi))
        .collect();
    assert_eq!(
        names,
        vec![("strong", -30), ("mid", -60), ("", -70), ("weak", -90)]
    );
    // A second scan replaces the list.
    w.start();
    w.finish([("only", -50)]);
    assert_eq!(w.networks().len(), 1);
}

#[test]
fn a_long_name_is_cut_on_a_character_boundary_and_ends_with_a_dot() {
    let font = small();
    let wide = 205;
    let short = fit_ssid(&font, "home", wide);
    assert_eq!(short.as_str(), "home");
    assert_eq!(fit_ssid(&font, "", wide).as_str(), "<hidden>");
    let long = fit_ssid(
        &font,
        "a-very-long-network-name-that-cannot-possibly-fit-here",
        wide,
    );
    assert!(
        long.as_str().ends_with('.') && long.as_str().len() < 40,
        "{}",
        long.as_str()
    );
    // Multi-byte characters are never split.
    let accents = fit_ssid(
        &font,
        "Café Zürich Gästenetzwerk öffentlich und sehr sehr lang",
        120,
    );
    assert!(accents.as_str().ends_with('.'));
    assert!(std::str::from_utf8(accents.as_str().as_bytes()).is_ok());
    // Nothing fits: just the dot.
    assert_eq!(fit_ssid(&font, "wide", 1).as_str(), ".");
}

#[test]
fn the_wifi_box_draws_every_state_inside_its_box() {
    let mut mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut fb = EpdFrameBuffer::<Ed047tc1>::new(&mut mem).unwrap();
    fb.set_rotation(Rotation::Deg270);
    let mut w = WifiApp::new();
    let dir = std::env::var("UI_DUMP").unwrap_or_default();
    let shot = |fb: &EpdFrameBuffer<'_, Ed047tc1>, name: &str| {
        if dir.is_empty() {
            return;
        }
        let mut out = Vec::new();
        for y in WIFI_Y..WIFI_Y + WIFI_H {
            for x in WIFI_X..WIFI_X + WIFI_W {
                out.push(fb.logical_pixel(x as u32, y as u32).unwrap() * 17);
            }
        }
        std::fs::write(format!("{dir}/{name}.raw"), out).unwrap();
    };
    let states: Vec<(&str, WifiDraw, Vec<(&str, i32)>)> = vec![
        ("wifi_scanning", WifiDraw::Scanning, vec![]),
        ("wifi_failed", WifiDraw::Failed("SCAN FAILED"), vec![]),
        ("wifi_none", WifiDraw::Results, vec![]),
        (
            "wifi_list",
            WifiDraw::Results,
            vec![
                ("melastmohican", -30),
                ("CenturyLink7149", -68),
                ("NTGR_VMB_3994174248", -84),
                ("", -72),
                ("Ring-f86783", -80),
                ("kager fast wifi", -73),
                ("NemoCom", -73),
                ("BRICAY 2.4", -87),
                ("ninth network", -90),
                ("tenth network", -91),
            ],
        ),
    ];
    for (name, what, found) in states {
        w.start();
        w.finish(found);
        fb.fill(0xF);
        fb.clear_dirty();
        draw_wifi(&mut fb, &w, what);
        shot(&fb, name);
        let dirty = fb.dirty().unwrap();
        let want = fb.rect_to_panel(Rect::new(
            WIFI_X as u32,
            WIFI_Y as u32,
            WIFI_W as u32,
            WIFI_H as u32,
        ));
        assert_eq!(
            (dirty.w, dirty.h),
            (want.w, want.h),
            "{name} stays in its box"
        );
    }
}
