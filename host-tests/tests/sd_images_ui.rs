//! Tests of the `mod ui` block of `examples/sd_images.rs`: finding the images on the card, the log
//! file, reading whole files, the pager and the pixel conversion.

mod support;

use embedded_sdmmc::{
    Block, BlockCount, BlockDevice, BlockIdx, Mode, TimeSource, Timestamp, VolumeIdx, VolumeManager,
};
use host_tests::epdpi::prelude::*;
use host_tests::sd_images::*;
use std::cell::RefCell;
use std::rc::Rc;

/// A RAM disk, shared so the test can put files on it before the store sees it.
#[derive(Clone)]
struct Ram(Rc<RefCell<Vec<u8>>>);

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
        let data = self.0.borrow();
        for (i, block) in blocks.iter_mut().enumerate() {
            let o = (start.0 as usize + i) * 512;
            block.contents.copy_from_slice(&data[o..o + 512]);
        }
        Ok(())
    }
    fn write(&self, blocks: &[Block], start: BlockIdx) -> Result<(), Fault> {
        let mut data = self.0.borrow_mut();
        for (i, block) in blocks.iter().enumerate() {
            let o = (start.0 as usize + i) * 512;
            data[o..o + 512].copy_from_slice(&block.contents);
        }
        Ok(())
    }
    fn num_blocks(&self) -> Result<BlockCount, Fault> {
        Ok(BlockCount((self.0.borrow().len() / 512) as u32))
    }
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

/// A card with these files and folders in the root.
fn card(files: &[(&str, &[u8])], dirs: &[&str]) -> Store<Ram, Time> {
    let ram = Ram(Rc::new(RefCell::new(support::fat32_image())));
    let mgr = VolumeManager::new(ram.clone(), Time);
    let volume = mgr.open_raw_volume(VolumeIdx(0)).unwrap();
    let root = mgr.open_root_dir(volume).unwrap();
    for (name, data) in files {
        let file = mgr
            .open_file_in_dir(root, *name, Mode::ReadWriteCreate)
            .unwrap();
        if !data.is_empty() {
            mgr.write(file, data).unwrap();
        }
        mgr.close_file(file).unwrap();
    }
    for dir in dirs {
        mgr.make_dir_in_dir(root, *dir).unwrap();
    }
    mgr.close_dir(root).unwrap();
    mgr.close_volume(volume).unwrap();
    Store::new(ram, Time)
}

fn names(slots: [Option<ShortName>; 2]) -> [Option<String>; 2] {
    slots.map(|slot| slot.map(|name| name.as_str().to_string()))
}

#[test]
fn finds_the_paper_images_first_and_in_name_order() {
    let mut store = card(
        &[
            ("ZED.PNG", b"x"),
            ("PAPER~34.PNG", b"x"),
            ("A.PNG", b"x"),
            ("PAPER~26.PNG", b"x"),
            ("NOTES.TXT", b"x"),
        ],
        &[],
    );
    assert_eq!(
        names(store.find_images().unwrap()),
        [Some("PAPER~26.PNG".into()), Some("PAPER~34.PNG".into())]
    );
}

#[test]
fn other_pngs_fill_the_slots_when_there_are_too_few_paper_ones() {
    let mut store = card(
        &[("B.PNG", b"x"), ("PAPER~26.PNG", b"x"), ("A.PNG", b"x")],
        &[],
    );
    assert_eq!(
        names(store.find_images().unwrap()),
        [Some("PAPER~26.PNG".into()), Some("A.PNG".into())]
    );
}

#[test]
fn one_image_leaves_the_second_slot_empty() {
    let mut store = card(&[("ONLY.PNG", b"x")], &[]);
    assert_eq!(
        names(store.find_images().unwrap()),
        [Some("ONLY.PNG".into()), None]
    );
}

#[test]
fn no_png_leaves_both_slots_empty() {
    let mut store = card(&[("NOTES.TXT", b"x"), ("PAPER.JPG", b"x")], &[]);
    assert_eq!(names(store.find_images().unwrap()), [None, None]);
}

#[test]
fn folders_and_empty_files_are_not_images() {
    let mut store = card(&[("EMPTY.PNG", b""), ("REAL.PNG", b"x")], &["DIR.PNG"]);
    assert_eq!(
        names(store.find_images().unwrap()),
        [Some("REAL.PNG".into()), None]
    );
}

#[test]
fn a_blank_card_is_not_a_volume() {
    let ram = Ram(Rc::new(RefCell::new(vec![0u8; 81_920 * 512])));
    let mut store = Store::new(ram, Time);
    assert_eq!(store.card_bytes(), Some(81_920 * 512));
    assert_eq!(store.find_images().err(), Some(StoreError::Mount));
    assert_eq!(store.append_log().err(), Some(StoreError::Mount));
}

#[test]
fn the_log_grows_by_one_line_each_time() {
    let mut store = card(&[], &[]);
    store.append_log().unwrap();
    assert_eq!(store.read_log().unwrap(), LOG_LINE);
    store.append_log().unwrap();
    let both = [LOG_LINE, LOG_LINE].concat();
    assert_eq!(store.read_log().unwrap(), both);
}

#[test]
fn reading_a_missing_file_says_not_found() {
    let mut store = card(&[], &[]);
    assert_eq!(
        store.read_file("NOPE.PNG").err(),
        Some(StoreError::NotFound)
    );
    assert_eq!(store.read_log().err(), Some(StoreError::NotFound));
}

#[test]
fn a_file_over_many_clusters_comes_back_whole() {
    // 40,000 bytes is 79 clusters of 512 bytes.
    let data: Vec<u8> = (0..40_000u32).map(|i| (i * 7 + i / 251) as u8).collect();
    let mut store = card(&[("BIG.PNG", &data)], &[]);
    assert_eq!(store.read_file("BIG.PNG").unwrap(), data);
    // Every operation closes what it opened: it can be done again and again.
    for _ in 0..20 {
        assert_eq!(store.read_file("BIG.PNG").unwrap().len(), data.len());
    }
}

#[test]
fn the_pager_wraps_both_ways() {
    let mut pager = Pager::new(2);
    assert_eq!(pager.current(), 0);
    assert_eq!(pager.next(), 1);
    assert_eq!(pager.next(), 0);
    assert_eq!(pager.previous(), 1);
    assert_eq!(pager.previous(), 0);

    // With more than two the direction shows.
    let mut pager = Pager::new(3);
    assert_eq!(pager.previous(), 2);
    assert_eq!(pager.previous(), 1);
    assert_eq!(pager.next(), 2);
    assert_eq!(pager.next(), 0);
}

#[test]
fn gray_is_luma_over_white() {
    assert_eq!(gray_over_white(0, 0, 0, 255), 0);
    assert_eq!(gray_over_white(255, 255, 255, 255), 255);
    // Fully transparent is the white the sketch fills the screen with, whatever the color.
    assert_eq!(gray_over_white(0, 0, 0, 0), 255);
    // Half transparent black is mid gray.
    let half = gray_over_white(0, 0, 0, 128);
    assert!((126..=129).contains(&half), "{half}");
    // Green counts most, blue least.
    assert!(gray_over_white(0, 255, 0, 255) > gray_over_white(255, 0, 0, 255));
    assert!(gray_over_white(255, 0, 0, 255) > gray_over_white(0, 0, 255, 255));
}

#[test]
fn quantize_spans_the_16_levels() {
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(quantize(0, x, y), 0);
            assert_eq!(quantize(255, x, y), 15);
        }
    }
}

fn landscape(mem: &mut Vec<u8>) -> Fb<'_> {
    let mut fb = EpdFrameBuffer::<Ed047tc1>::new(mem).unwrap();
    fb.set_rotation(Rotation::Deg0);
    fb
}

#[test]
fn rgba_fills_the_landscape_screen() {
    let mut mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut fb = landscape(&mut mem);
    let (w, h) = fb.logical_size();
    assert_eq!((w, h), (960, 540));
    let rgba: Vec<u8> = [0u8, 0, 0, 255].repeat((w * h) as usize);
    draw_rgba(&mut fb, &rgba, w, h);
    assert_eq!(fb.logical_pixel(0, 0), Some(0));
    assert_eq!(fb.logical_pixel(w - 1, h - 1), Some(0));
    assert_eq!(fb.logical_pixel(500, 300), Some(0));
}

#[test]
fn a_smaller_or_bigger_image_is_placed_at_the_top_left_and_cut() {
    let mut mem = vec![0xFFu8; Ed047tc1::FB_BYTES];
    let mut fb = landscape(&mut mem);
    // 2 by 2, black.
    draw_rgba(&mut fb, &[0, 0, 0, 255].repeat(4), 2, 2);
    assert_eq!(fb.logical_pixel(1, 1), Some(0));
    assert_eq!(fb.logical_pixel(2, 2), Some(15));
    // Wider and taller than the screen: no panic, the part that fits is drawn.
    let (w, h) = (1000u32, 600u32);
    draw_rgba(
        &mut fb,
        &[255, 255, 255, 255].repeat((w * h) as usize),
        w,
        h,
    );
    assert_eq!(fb.logical_pixel(1, 1), Some(15));
}

#[test]
fn transparent_pixels_come_out_white() {
    let mut mem = vec![0u8; Ed047tc1::FB_BYTES];
    let mut fb = landscape(&mut mem);
    draw_rgba(&mut fb, &[10, 20, 30, 0].repeat(4), 2, 2);
    assert_eq!(fb.logical_pixel(0, 0), Some(15));
}

#[test]
fn status_screen_has_text_and_the_build_label() {
    let mut mem = vec![0u8; Ed047tc1::FB_BYTES];
    let mut fb = landscape(&mut mem);
    draw_status(
        &mut fb,
        "PNG not found",
        "/PaperDIY_01.png",
        "Rust: epdpi blocking",
    );
    let (w, h) = fb.logical_size();
    let dark_in =
        |y0: u32, y1: u32| (y0..y1).any(|y| (0..w).any(|x| fb.logical_pixel(x, y) == Some(0)));
    assert!(dark_in(h / 2 - 60, h / 2 - 10), "title");
    assert!(dark_in(h / 2 + 15, h / 2 + 60), "detail");
    assert!(dark_in(h - 55, h - 25), "label");
    assert_eq!(fb.logical_pixel(2, 2), Some(15), "the rest is white");
}

#[test]
fn the_label_sits_bottom_left_on_a_white_box() {
    let mut mem = vec![0u8; Ed047tc1::FB_BYTES];
    let mut fb = landscape(&mut mem);
    draw_rgba(&mut fb, &[0, 0, 0, 255].repeat(960 * 540), 960, 540);
    draw_label(&mut fb, "Rust: epdpi blocking");
    let (_, h) = fb.logical_size();
    assert_eq!(fb.logical_pixel(190, h - 3), Some(15), "box is white");
    assert!(
        (0..200).any(|x| (h - 26..h).any(|y| fb.logical_pixel(x, y) == Some(0))),
        "text"
    );
    assert_eq!(
        fb.logical_pixel(300, h - 3),
        Some(0),
        "image beyond the box is untouched"
    );
    assert_eq!(
        fb.logical_pixel(10, h - 40),
        Some(0),
        "above the box is untouched"
    );
}
