//! Tests of `common/src/render.rs`: the text the font can draw, the font's widths, and the four
//! screens of the reader drawn into memory. With `UI_DUMP=/some/dir` the tests also write each screen
//! as a `.pgm` picture there, to look at.

mod support;

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use m5paperdiy_common::html::parse;
use m5paperdiy_common::layout::{Layout, TextMetrics};
use m5paperdiy_common::render::{
    BOOKS_PER_SCREEN, ENTRIES_PER_SCREEN, Geometry, Serif, draw_book_list, draw_contents,
    draw_notice, draw_page, glyph_text,
};

const G: Geometry = Geometry::PORTRAIT;

/// The screen in memory: one gray level 0 (black) to 15 (white) per pixel.
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn white() -> Self {
        Self {
            width: G.width,
            height: G.height,
            pixels: vec![15; (G.width * G.height) as usize],
        }
    }

    fn at(&self, x: i32, y: i32) -> u8 {
        self.pixels[(y as u32 * self.width + x as u32) as usize]
    }

    /// The smallest box that holds every dark pixel inside `rows`, as `(left, top, right, bottom)`.
    fn ink(&self, rows: std::ops::Range<i32>) -> Option<(i32, i32, i32, i32)> {
        let mut found: Option<(i32, i32, i32, i32)> = None;
        for y in rows {
            for x in 0..self.width as i32 {
                if self.at(x, y) < 8 {
                    let b = found.get_or_insert((x, y, x, y));
                    b.0 = b.0.min(x);
                    b.1 = b.1.min(y);
                    b.2 = b.2.max(x);
                    b.3 = b.3.max(y);
                }
            }
        }
        found
    }

    fn dump(&self, name: &str) {
        let Some(dir) = support::dump_dir() else {
            return;
        };
        std::fs::create_dir_all(&dir).unwrap();
        let mut file = format!("P5\n{} {}\n255\n", self.width, self.height).into_bytes();
        file.extend(self.pixels.iter().map(|&p| p * 17));
        std::fs::write(format!("{dir}/{name}.pgm"), file).unwrap();
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

const SAMPLE: &str = "<html><body><h1>Chapter One: The \u{201c}Quick\u{201d} Fox</h1>\
<p>It was a bright cold day in April, and the clocks were striking thirteen. Winston Smith, his chin \
nuzzled into his breast in an effort to escape the vile wind, slipped quickly through the glass doors \
of Victory Mansions \u{2014} though not quickly enough to prevent a swirl of gritty dust from entering \
along with him. <b>Bold words</b> and <i>italic words</i> sit in the same paragraph.</p>\
<p>The hallway smelt of boiled cabbage and old rag mats.\u{2026} It\u{2019}s a caf\u{e9}.</p></body></html>";

// ---------------------------------------------------------------------------------------------

#[test]
fn the_font_has_a_glyph_for_everything_that_is_drawn() {
    assert_eq!(
        glyph_text("plain Latin-1: caf\u{e9} \u{a9}"),
        "plain Latin-1: caf\u{e9} \u{a9}"
    );
    assert_eq!(
        glyph_text("it\u{2019}s \u{201c}q\u{201d} \u{2014} \u{2013} \u{2026} \u{2022}"),
        "it's \"q\" - - ... *"
    );
    assert_eq!(
        glyph_text("a\u{a0}b\u{ad}c\u{fb01}\u{fb02}\u{2122}"),
        "a bcfiflTM"
    );
    assert_eq!(
        glyph_text("\u{65e5}\u{672c}x"),
        "??x",
        "other scripts become ?"
    );
}

#[test]
fn widths_come_from_the_font() {
    let font = Serif;
    assert_eq!(font.line_height(), 30);
    assert_eq!(font.text_width("", false, false), 0);
    assert!(font.space_width() > 0);
    let regular = font.text_width("Reading", false, false);
    let bold = font.text_width("Reading", true, false);
    assert!(
        regular > 30 && bold > regular,
        "bold is wider: {regular} {bold}"
    );
    // Italic has no font of its own, so it measures like regular.
    assert_eq!(font.text_width("Reading", false, true), regular);
    // A word with a curly quote measures as the plain one does.
    assert_eq!(
        font.text_width("it\u{2019}s", false, false),
        font.text_width("it's", false, false)
    );
    assert!(
        font.text_width("\u{65e5}", false, false) > 0,
        "a glyph that is missing still has a width"
    );
}

#[test]
fn a_page_of_text_stays_inside_the_margins() {
    let layout = Layout::new(
        parse(SAMPLE.as_bytes(), ""),
        &Serif,
        G.page_width(),
        G.page_height(),
        &|_| None,
    );
    assert_eq!(layout.page_count(), 1);
    let mut canvas = Canvas::white();
    draw_page(&mut canvas, &G, &layout, 0);
    canvas.dump("page");
    let (left, top, right, bottom) = canvas.ink(0..G.height as i32).expect("something is drawn");
    // A glyph may reach a pixel past its cell, so allow one.
    assert!(left >= G.margin_left as i32 - 1, "left edge {left}");
    assert!(
        right <= (G.width - G.margin_right) as i32,
        "right edge {right}"
    );
    assert!(top >= G.margin_top as i32 - 1, "top edge {top}");
    assert!(
        bottom < G.height as i32 - 100,
        "this short chapter ends well above the bottom: {bottom}"
    );
    // The heading is centered: its ink is symmetric around the middle of the text area.
    let (hl, _, hr, _) = canvas.ink(35..70).unwrap();
    let centre = (hl + hr) / 2;
    assert!((centre - 270).abs() <= 12, "heading centre {centre}");
}

#[test]
fn a_long_chapter_fills_whole_pages_down_to_the_last_line() {
    let words: Vec<&str> = (0..4000).map(|_| "readable").collect();
    let html = format!("<p>{}</p>", words.join(" "));
    let layout = Layout::new(
        parse(html.as_bytes(), ""),
        &Serif,
        G.page_width(),
        G.page_height(),
        &|_| None,
    );
    assert!(layout.page_count() > 3);
    let mut canvas = Canvas::white();
    draw_page(&mut canvas, &G, &layout, 1);
    let (_, top, _, bottom) = canvas.ink(0..G.height as i32).unwrap();
    // 925 / 30 = 30 lines, so a full page ends within one line of the bottom.
    assert!(top <= 45, "starts at the top: {top}");
    assert!(
        bottom >= G.height as i32 - 45,
        "ends near the bottom: {bottom}"
    );
    assert!(
        bottom < G.height as i32,
        "and stays on the screen: {bottom}"
    );
}

fn titles() -> Vec<String> {
    vec![
        "Gulliver's Travels".to_string(),
        "A Journey to the Centre of the Earth, with an Unusually Long Title That Needs Several Lines".to_string(),
        "Short".to_string(),
        "Fourth".to_string(),
        "Fifth \u{201c}quoted\u{201d}".to_string(),
        "Sixth, on the second screen".to_string(),
        "Seventh".to_string(),
    ]
}

/// The top of the cell `slot` of a list shown `per_screen` to a screen.
fn cell_top(slot: usize, per_screen: usize) -> i32 {
    (G.margin_top + slot as u32 * (G.page_height() / per_screen as u32)) as i32
}

#[test]
fn the_book_list_boxes_the_chosen_book_and_shows_five_to_a_screen() {
    let titles = titles();
    let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
    let cell = (G.page_height() / BOOKS_PER_SCREEN as u32) as i32;

    // The first screen with book 1 chosen.
    let mut canvas = Canvas::white();
    draw_book_list(&mut canvas, &G, &refs, 1);
    canvas.dump("list");
    // The box is 5 px thick, 10 px inside the cell: its top left corner is black, and the same
    // place in another cell is white.
    let (bx, by) = (G.margin_left as i32, cell_top(1, 5) + 10);
    assert_eq!(canvas.at(bx, by), 0, "corner of the box of book 1");
    assert_eq!(canvas.at(bx + 4, by + 4), 0, "the box is 5 thick");
    assert_eq!(canvas.at(bx + 5, by + 5), 15, "and no thicker");
    assert_eq!(
        canvas.at(bx, cell_top(0, 5) + 10),
        15,
        "no box round book 0"
    );
    // Every cell has text, the sixth book is not on this screen: the last cell is the fifth.
    for slot in 0..5 {
        let rows = cell_top(slot, 5) + 12..cell_top(slot, 5) + cell - 12;
        assert!(canvas.ink(rows).is_some(), "cell {slot} has text");
    }
    // The long title wraps inside its cell instead of running into the next one.
    let (_, top, _, bottom) = canvas
        .ink(cell_top(1, 5) + 16..cell_top(2, 5) - 16)
        .unwrap();
    assert!(
        top >= cell_top(1, 5) && bottom < cell_top(2, 5),
        "wrapped title {top}..{bottom}"
    );

    // Choosing book 5 moves to the second screen: it starts with books 5 and 6, and nothing below.
    let mut canvas = Canvas::white();
    draw_book_list(&mut canvas, &G, &refs, 5);
    assert_eq!(
        canvas.at(bx, cell_top(0, 5) + 10),
        0,
        "box round the chosen book, first cell"
    );
    assert!(
        canvas
            .ink(cell_top(1, 5) + 12..cell_top(2, 5) - 12)
            .is_some(),
        "book 6"
    );
    assert!(
        canvas.ink(cell_top(2, 5) + 12..G.height as i32).is_none(),
        "the screen is not full"
    );
}

#[test]
fn the_table_of_contents_indents_by_level_and_boxes_the_chosen_entry() {
    let entries = [
        ("Part One", 0u8),
        ("Chapter 1", 1),
        ("Section 1.1", 2),
        ("Section 1.1.1 deeper", 5),
        ("Part Two", 0),
        ("Chapter 2", 1),
        ("Seventh entry, on the next screen", 0),
    ];
    let cell = (G.page_height() / ENTRIES_PER_SCREEN as u32) as i32;
    let mut canvas = Canvas::white();
    draw_contents(&mut canvas, &G, entries.iter().copied(), 4);
    canvas.dump("contents");
    // The box round entry 4 is 3 px thick, 7 px inside the cell.
    let (bx, by) = (G.margin_left as i32, cell_top(4, 6) + 7);
    assert_eq!(canvas.at(bx, by), 0);
    assert_eq!(canvas.at(bx + 2, by + 2), 0);
    assert_eq!(canvas.at(bx + 3, by + 3), 15);
    // Text starts further right for deeper levels, and levels beyond 3 are not indented more.
    let left_of = |slot: usize| {
        canvas
            .ink(cell_top(slot, 6) + 10..cell_top(slot, 6) + cell - 10)
            .map(|b| b.0)
            .unwrap()
    };
    let (l0, l1, l2, l5) = (left_of(0), left_of(1), left_of(2), left_of(3));
    assert!(l1 > l0 && l2 > l1, "indent grows: {l0} {l1} {l2}");
    assert!(l5 - l2 <= 14 + 1, "level 5 is capped at level 3: {l2} {l5}");
    // Six to a screen: the seventh entry is not drawn.
    assert!(canvas.ink(cell_top(5, 6) + cell..G.height as i32).is_none());
}

#[test]
fn the_notice_is_a_boxed_message_in_the_middle_of_the_screen() {
    let mut canvas = Canvas::white();
    // Something underneath that the notice must cover.
    for p in canvas.pixels.iter_mut() {
        *p = 6;
    }
    draw_notice(
        &mut canvas,
        &G,
        &["Reached the limit of the book", "Use the SELECT button"],
    );
    canvas.dump("notice");
    assert_eq!(canvas.at(0, 0), 6, "the rest of the screen is untouched");
    assert_eq!(canvas.at(270, 480), 15, "inside the box it is white");
    let (left, top, right, bottom) = {
        // The frame is black; find it by scanning for the outermost black pixels near the centre.
        let mut found = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for y in 0..G.height as i32 {
            for x in 0..G.width as i32 {
                if canvas.at(x, y) == 0 {
                    found = (
                        found.0.min(x),
                        found.1.min(y),
                        found.2.max(x),
                        found.3.max(y),
                    );
                }
            }
        }
        found
    };
    assert!(
        ((left + right) / 2 - 270).abs() <= 1,
        "centered across: {left}..{right}"
    );
    assert!(
        ((top + bottom) / 2 - 480).abs() <= 1,
        "centered down: {top}..{bottom}"
    );
}

/// Real books, from `EPUB_TEST_FILES`: draws a page from the middle of the first chapter that has
/// at least three pages, for each book, to look at with `UI_DUMP`.
#[test]
#[ignore = "needs real books: set EPUB_TEST_FILES"]
fn pages_of_real_books_are_drawn() {
    use m5paperdiy_common::epub::{Book, dir_of};
    use m5paperdiy_common::storage::{DirEntry, Storage, StorageError};
    struct Disk;
    impl Storage for Disk {
        fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
            use std::io::{Read, Seek, SeekFrom};
            let mut f = std::fs::File::open(path).map_err(|_| StorageError::NotFound)?;
            f.seek(SeekFrom::Start(offset))
                .map_err(|_| StorageError::Io)?;
            f.read(buf).map_err(|_| StorageError::Io)
        }
        fn size(&self, path: &str) -> Result<u64, StorageError> {
            std::fs::metadata(path)
                .map(|m| m.len())
                .map_err(|_| StorageError::NotFound)
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Unsupported)
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
            Err(StorageError::Unsupported)
        }
        fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            Err(StorageError::Unsupported)
        }
        fn remove(&self, _: &str) -> Result<(), StorageError> {
            Err(StorageError::Unsupported)
        }
    }
    let files = std::env::var("EPUB_TEST_FILES").expect("set EPUB_TEST_FILES");
    for (n, path) in files.split(':').filter(|p| !p.is_empty()).enumerate() {
        let book = Book::open(&Disk, path).unwrap();
        for i in 0..book.spine().len() {
            let data = book.read_spine(&Disk, i).unwrap();
            let layout = Layout::new(
                parse(&data, dir_of(&book.spine()[i])),
                &Serif,
                G.page_width(),
                G.page_height(),
                &|_| None,
            );
            if layout.page_count() >= 3 {
                let mut canvas = Canvas::white();
                draw_page(&mut canvas, &G, &layout, 1);
                canvas.dump(&format!("real-book-{n}"));
                let ink = canvas.ink(0..G.height as i32).expect("ink");
                assert!(
                    ink.0 >= 9 && ink.2 <= 530,
                    "{path}: inside the margins {ink:?}"
                );
                println!(
                    "{path}: spine {i}, {} pages, page 1 drawn",
                    layout.page_count()
                );
                break;
            }
        }
    }
}
