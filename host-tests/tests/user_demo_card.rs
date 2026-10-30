//! Tests of the card widget of `examples/user_demo.rs` against a RAM disk holding a real FAT32 volume.

mod support;

use embedded_sdmmc::{
    Block, BlockCount, BlockDevice, BlockIdx, TimeSource, Timestamp, VolumeIdx, VolumeManager,
};
use host_tests::epdpi::prelude::*;
use host_tests::user_demo::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The RAM disk's bytes and the switches that make it misbehave, shared with the test.
struct Switches {
    data: RefCell<Vec<u8>>,
    present: Cell<bool>,
    fail_writes: Cell<bool>,
    corrupt_reads: Cell<bool>,
}

/// A RAM disk loaded from a real FAT32 image.
struct Ram(Rc<Switches>);

#[derive(Debug)]
struct Fault;

impl core::fmt::Display for Fault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("fault")
    }
}

impl core::error::Error for Fault {}

impl BlockDevice for Ram {
    type Error = Fault;
    fn read(&self, blocks: &mut [Block], start: BlockIdx) -> Result<(), Fault> {
        let data = self.0.data.borrow();
        for (i, block) in blocks.iter_mut().enumerate() {
            let o = (start.0 as usize + i) * 512;
            block.contents.copy_from_slice(&data[o..o + 512]);
            // Past the root directory (sector 3328): the test file's data.
            if self.0.corrupt_reads.get() && start.0 > 3328 {
                block.contents[7] ^= 0xFF;
            }
        }
        Ok(())
    }
    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Fault> {
        if self.0.fail_writes.get() && start.0 > 3328 {
            return Err(Fault);
        }
        let mut data = self.0.data.borrow_mut();
        for (i, block) in blocks.iter().enumerate() {
            let o = (start.0 as usize + i) * 512;
            data[o..o + 512].copy_from_slice(&block.contents);
        }
        Ok(())
    }
    fn num_blocks(&self) -> Result<BlockCount, Fault> {
        Ok(BlockCount((self.0.data.borrow().len() / 512) as u32))
    }
}

impl Card for Ram {
    fn kind(&self) -> Option<&'static str> {
        self.0.present.get().then_some("SDHC/SDXC")
    }
    fn forget(&self) {}
}

struct Time;
impl TimeSource for Time {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 9,
            zero_indexed_day: 28,
            hours: 12,
            minutes: 0,
            seconds: 0,
        }
    }
}

fn app() -> (CardApp<Ram, Time>, Rc<Switches>) {
    let img = support::fat32_image();
    let switches = Rc::new(Switches {
        data: RefCell::new(img),
        present: Cell::new(true),
        fail_writes: Cell::new(false),
        corrupt_reads: Cell::new(false),
    });
    (CardApp::new(Ram(switches.clone()), Time), switches)
}

/// Clicks B, then steps the app with a clock that moves 1000 us per call until it draws again.
fn run_test(app: &mut CardApp<Ram, Time>, now_ms: u64) -> (Vec<CardDraw>, u32) {
    let ticks = Cell::new(0u64);
    let mut clock = || {
        ticks.set(ticks.get() + 1000);
        ticks.get()
    };
    let mut draws = Vec::new();
    draws.extend(app.update(now_ms, true, true, &mut clock));
    let mut steps = 0;
    loop {
        if let Some(draw) = app.update(now_ms, true, false, &mut clock) {
            draws.push(draw);
            return (draws, steps);
        }
        steps += 1;
        assert!(steps < 300, "the test never ends");
    }
}

/// How many entries the root directory of the disk holds, read with a fresh manager.
fn root_entries(switches: &Switches) -> usize {
    let ram = Ram(Rc::new(Switches {
        data: RefCell::new(switches.data.borrow().clone()),
        present: Cell::new(true),
        fail_writes: Cell::new(false),
        corrupt_reads: Cell::new(false),
    }));
    let mgr = VolumeManager::new(ram, Time);
    let vol = mgr.open_raw_volume(VolumeIdx(0)).unwrap();
    let dir = mgr.open_root_dir(vol).unwrap();
    let mut n = 0;
    mgr.iterate_dir(dir, |_| {
        n += 1;
        std::ops::ControlFlow::Continue(())
    })
    .unwrap();
    n
}

#[test]
fn mounts_a_fat_card_and_reports_kind_and_size() {
    let (mut a, _sw) = app();
    assert!(a.begin(0, true) == CardDraw::Info);
    let info = a.info();
    assert!(info.status == Status::Mounted);
    assert_eq!(info.kind, "SDHC/SDXC");
    assert_eq!(info.bytes, 40 * 1024 * 1024);
}

#[test]
fn no_card_when_detect_is_high_and_a_card_appears_within_half_a_second() {
    let (mut a, _sw) = app();
    a.begin(0, false);
    assert!(a.info().status == Status::NoCard);
    // Same state: nothing to draw, however often it is polled.
    for t in (0..2000).step_by(5) {
        assert!(a.update(t, false, false, || 0).is_none(), "t={t}");
    }
    // Inserted: noticed at the next 500 ms poll, drawn once.
    let mut drawn = None;
    for t in (2000..3000).step_by(5) {
        if let Some(d) = a.update(t, true, false, || 0) {
            drawn = Some((t, d));
            break;
        }
    }
    let (t, d) = drawn.expect("card noticed");
    assert!(t <= 2505 && d == CardDraw::Info, "noticed at {t}");
    assert!(a.info().status == Status::Mounted);
}

#[test]
fn removing_the_card_resets_the_result_and_redraws() {
    let (mut a, _sw) = app();
    a.begin(0, true);
    run_test(&mut a, 100);
    assert!(a.result().success);
    let mut drawn = None;
    for t in (600..1200).step_by(5) {
        if let Some(d) = a.update(t, false, false, || 0) {
            drawn = Some(d);
            break;
        }
    }
    assert!(drawn == Some(CardDraw::Info));
    assert!(a.info().status == Status::NoCard);
    assert!(!a.result().success && a.result().message.is_empty());
}

#[test]
fn the_speed_test_runs_one_chunk_at_a_time_and_reports_both_speeds() {
    let (mut a, _sw) = app();
    a.begin(0, true);
    let (draws, steps) = run_test(&mut a, 100);
    assert!(draws[0] == CardDraw::Testing && draws.last() == Some(&CardDraw::Info));
    assert_eq!(draws.len(), 2);
    // 1 MiB in 16 KiB chunks: 64 writes, the close, 64 reads. The first update starts it.
    assert_eq!(
        steps, 127,
        "64 writes and 63 reads return nothing, the 64th read ends it"
    );
    let r = a.result();
    assert!(r.success && r.message == "OK");
    assert!(
        r.write_mibs > 0.0 && r.read_mibs > 0.0,
        "{} {}",
        r.write_mibs,
        r.read_mibs
    );
}

#[test]
fn the_test_file_is_deleted_afterwards() {
    let (mut a, sw) = app();
    a.begin(0, true);
    assert_eq!(root_entries(&sw), 0);
    run_test(&mut a, 100);
    assert!(a.result().success);
    assert_eq!(root_entries(&sw), 0, "PDT file left behind");
}

#[test]
fn a_failing_write_ends_the_test_with_a_message_and_cleans_up() {
    let (mut a, sw) = app();
    a.begin(0, true);
    sw.fail_writes.set(true);
    let (draws, _) = run_test(&mut a, 100);
    assert!(draws.last() == Some(&CardDraw::Info));
    assert!(!a.result().success);
    assert_eq!(a.result().message, "WRITE FAILED");
    sw.fail_writes.set(false);
    // The failed test must not leave a file or an open handle: a new test runs fine.
    let (_, _) = run_test(&mut a, 200);
    assert!(a.result().success, "{}", a.result().message);
}

#[test]
fn corrupt_data_on_read_back_is_caught() {
    let (mut a, sw) = app();
    a.begin(0, true);
    sw.corrupt_reads.set(true);
    let (draws, _) = run_test(&mut a, 100);
    assert!(draws.last() == Some(&CardDraw::Info));
    assert_eq!(a.result().message, "READ VERIFY FAILED");
    sw.corrupt_reads.set(false);
    run_test(&mut a, 200);
    assert!(a.result().success, "{}", a.result().message);
}

#[test]
fn partials_escalate_every_fortieth_and_clicks_wait_for_both_buttons_up() {
    let mut p = Partials::default();
    let full: Vec<u32> = (1..=120)
        .filter(|_| p.bump())
        .collect::<Vec<_>>()
        .iter()
        .map(|_| 0)
        .collect();
    assert_eq!(full.len(), 3);
    let mut p = Partials::default();
    let at: Vec<u32> = (1..=120).filter(|_| p.bump()).collect();
    assert_eq!(at, vec![40, 80, 120]);

    let mut c = Clicks::default();
    assert_eq!(c.update(false, false), (false, false));
    assert_eq!(c.update(false, true), (false, false), "down is not a click");
    assert_eq!(c.update(false, false), (false, true), "release is");
    assert_eq!(c.update(false, false), (false, false), "once");
    // B released while A is still down: wait for A to come up.
    c.update(true, true);
    assert_eq!(c.update(true, false), (false, false));
    assert_eq!(c.update(false, false), (true, true));
}

#[test]
fn the_card_box_draws_every_state() {
    let mut mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut fb = EpdFrameBuffer::<Ed047tc1>::new(&mut mem).unwrap();
    fb.set_rotation(Rotation::Deg270);
    let mounted = Info {
        status: Status::Mounted,
        kind: "SDHC/SDXC",
        bytes: 31_914_983_424,
    };
    let ok = BenchResult {
        success: true,
        write_mibs: 0.43,
        read_mibs: 0.55,
        message: "OK",
    };
    let states: [(&str, Info, BenchResult, CardDraw); 5] = [
        (
            "card_none",
            Info {
                status: Status::NoCard,
                kind: "",
                bytes: 0,
            },
            BenchResult::NONE,
            CardDraw::Info,
        ),
        (
            "card_failed",
            Info {
                status: Status::MountFailed,
                kind: "",
                bytes: 0,
            },
            BenchResult::NONE,
            CardDraw::Info,
        ),
        ("card_idle", mounted, BenchResult::NONE, CardDraw::Info),
        ("card_speeds", mounted, ok, CardDraw::Info),
        (
            "card_testing",
            mounted,
            BenchResult::NONE,
            CardDraw::Testing,
        ),
    ];
    let dir = std::env::var("UI_DUMP").unwrap_or_default();
    for (name, info, result, what) in states {
        fb.fill(0xF);
        fb.clear_dirty();
        draw_card(&mut fb, &info, &result, what);
        if !dir.is_empty() {
            let mut out = Vec::new();
            for y in CARD_Y..CARD_Y + CARD_H {
                for x in CARD_X..CARD_X + CARD_W {
                    out.push(fb.logical_pixel(x as u32, y as u32).unwrap() * 17);
                }
            }
            std::fs::write(format!("{dir}/{name}.raw"), out).unwrap();
        }
        // Nothing outside the box is touched.
        let dirty = fb.dirty().unwrap();
        let want = fb.rect_to_panel(Rect::new(
            CARD_X as u32,
            CARD_Y as u32,
            CARD_W as u32,
            CARD_H as u32,
        ));
        assert_eq!((dirty.w, dirty.h), (want.w, want.h), "{name}");
        fb.clear_dirty();
    }
}
