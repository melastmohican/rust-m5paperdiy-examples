//! Tests of the covers in the book list: the library reads the cover of each book on the screen,
//! scales it to the size of the Arduino list, keeps it for the session, and leaves the title alone
//! for a book with no cover or one that cannot be decoded. The books are made in the test with the
//! pictures of `fixtures/images/` (40 by 20, the left half black, the right half white), and the
//! card is a [`Storage`] in memory that counts its reads.

mod support;

use embedded_graphics::{Pixel, pixelcolor::Gray4, prelude::*};
use m5paperdiy_common::reader::{Action, App, Effect};
use m5paperdiy_common::render::{BOOKS_PER_SCREEN, Geometry, cover_size};
use m5paperdiy_common::sd::{BookEntry, BookLibrary};
use m5paperdiy_common::storage::{DirEntry, Storage, StorageError};
use std::cell::RefCell;
use std::collections::BTreeMap;

const G: Geometry = Geometry::PORTRAIT;

/// Files in memory, and how many reads each file has had.
#[derive(Default)]
struct Mem {
    files: BTreeMap<String, Vec<u8>>,
    reads: RefCell<BTreeMap<String, usize>>,
}

impl Storage for Mem {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        *self.reads.borrow_mut().entry(path.to_string()).or_default() += 1;
        let data = self.files.get(path).ok_or(StorageError::NotFound)?;
        let start = (offset as usize).min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files
            .get(path)
            .map(|d| d.len() as u64)
            .ok_or(StorageError::NotFound)
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

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/images/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A book of one chapter. `cover` is the fixture to use as its cover, and with `named` false the
/// package has the file but no `<meta name="cover">`.
fn book(cover: Option<&str>, named: bool) -> Vec<u8> {
    const CONTAINER: &str = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    let meta = if cover.is_some() && named {
        r#"<meta name="cover" content="cov"/>"#
    } else {
        ""
    };
    let item = if cover.is_some() {
        r#"<item id="cov" href="images/cover.png" media-type="image/png"/>"#
    } else {
        ""
    };
    let opf = format!(
        r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Covers</dc:title>{meta}</metadata><manifest><item id="c" href="ch.xhtml" media-type="application/xhtml+xml"/>{item}</manifest><spine><itemref idref="c"/></spine></package>"#
    );
    let picture = cover.map(fixture).unwrap_or_default();
    let mut files: Vec<(&str, &[u8])> = vec![
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/ch.xhtml", b"<html><body><p>text</p></body></html>"),
    ];
    if cover.is_some() {
        files.push(("OEBPS/images/cover.png", &picture));
    }
    support::zip_bytes(&files, &[])
}

/// A library of books `(path, book)`, titled `Book 0`, `Book 1`...
fn library(books: Vec<(&str, Vec<u8>)>) -> BookLibrary<Mem> {
    let mut mem = Mem::default();
    let mut entries = Vec::new();
    for (i, (path, data)) in books.into_iter().enumerate() {
        mem.files.insert(path.to_string(), data);
        entries.push(BookEntry {
            path: path.to_string(),
            title: format!("Book {i}"),
        });
    }
    BookLibrary::with_books(mem, entries, G)
}

struct Canvas {
    pixels: Vec<u8>,
}

impl Canvas {
    fn white() -> Self {
        Self {
            pixels: vec![15; (G.width * G.height) as usize],
        }
    }

    fn at(&self, x: u32, y: u32) -> u8 {
        self.pixels[(y * G.width + x) as usize]
    }

    /// Is there a pixel darker than 8 in the columns `cols` of the rows `rows`?
    fn has_ink(&self, cols: std::ops::Range<u32>, rows: std::ops::Range<u32>) -> bool {
        rows.into_iter()
            .any(|y| cols.clone().any(|x| self.at(x, y) < 8))
    }
}

impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        Size::new(G.width, G.height)
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
            if p.x >= 0 && p.y >= 0 && (p.x as u32) < G.width && (p.y as u32) < G.height {
                self.pixels[(p.y as u32 * G.width + p.x as u32) as usize] = color.luma();
            }
        }
        Ok(())
    }
}

/// Draws the list with book `selected` chosen.
fn draw_list(lib: &mut BookLibrary<Mem>, selected: usize) -> Canvas {
    let mut app = App::new();
    for _ in 0..selected {
        assert_eq!(app.press(Action::Down, lib), Effect::Redraw);
    }
    let mut canvas = Canvas::white();
    lib.draw(&app, Effect::Redraw, &mut canvas);
    canvas
}

fn cell() -> u32 {
    G.page_height() / BOOKS_PER_SCREEN as u32
}

fn row(slot: u32) -> u32 {
    G.margin_top + slot * cell()
}

#[test]
fn a_cover_is_drawn_scaled_at_the_place_of_the_list_and_the_others_are_not() {
    let mut lib = library(vec![
        ("A.EPU", book(Some("gray.png"), true)),
        ("B.EPU", book(None, true)),
        // The file is there and named, but is text.
        ("C.EPU", book(Some("not_an_image.txt"), true)),
        // A picture in the book that no meta names is not a cover.
        ("D.EPU", book(Some("gray.png"), false)),
    ]);
    // Book 3 is chosen so no box is round the first cells.
    let canvas = draw_list(&mut lib, 3);

    let (width, height) = cover_size(&G);
    let (x, y) = (G.margin_left + 20, row(0) + 20);
    // The left half of the picture is black, the right half white, whatever the scale.
    assert_eq!(canvas.at(x + 2, y + 2), 0, "black half");
    assert_eq!(
        canvas.at(x + width / 2 - 3, y + height - 3),
        0,
        "black half, far corner"
    );
    assert_eq!(canvas.at(x + width / 2 + 3, y + 2), 15, "white half");
    assert_eq!(
        canvas.at(x + width - 3, y + height - 3),
        15,
        "white half, far corner"
    );
    assert_eq!(canvas.at(x - 1, y + 5), 15, "left of the cover");
    assert_eq!(canvas.at(x + width, y + 5), 15, "right of the cover");

    // Title of the books with no cover usable: at the left, where the cover would be. The cover
    // of book 0 starts at x, so left of it only a title can have ink.
    let left = G.margin_left + 10..x;
    assert!(
        !canvas.has_ink(left.clone(), row(0) + 22..row(1) - 22),
        "book 0 title is after the cover"
    );
    for slot in [1, 2, 3] {
        assert!(
            canvas.has_ink(left.clone(), row(slot) + 22..row(slot + 1) - 22),
            "book {slot} shows only its title, at the left"
        );
    }
}

#[test]
fn only_the_books_on_the_screen_are_read_and_each_is_decoded_once() {
    let books: Vec<(&str, Vec<u8>)> = [
        "B0.EPU", "B1.EPU", "B2.EPU", "B3.EPU", "B4.EPU", "B5.EPU", "B6.EPU",
    ]
    .into_iter()
    .map(|path| (path, book(Some("gray.png"), true)))
    .collect();
    let paths: Vec<&str> = books.iter().map(|(p, _)| *p).collect();
    let mut lib = library(books);
    let reads = |lib: &BookLibrary<Mem>| -> Vec<usize> {
        paths
            .iter()
            .map(|p| lib.storage().reads.borrow().get(*p).copied().unwrap_or(0))
            .collect()
    };

    draw_list(&mut lib, 0);
    let first = reads(&lib);
    assert!(
        first[..5].iter().all(|&n| n > 0),
        "the 5 books on the screen are read: {first:?}"
    );
    assert_eq!(first[5..], [0, 0], "the books of the next screen are not");

    // The same screen again, with another book chosen: no read at all.
    draw_list(&mut lib, 3);
    assert_eq!(reads(&lib), first, "covers are kept for the session");

    // The next screen reads its two books and leaves the first five alone.
    draw_list(&mut lib, 5);
    let second = reads(&lib);
    assert_eq!(second[..5], first[..5]);
    assert!(second[5..].iter().all(|&n| n > 0));

    // Back on the first screen: still nothing new.
    draw_list(&mut lib, 0);
    assert_eq!(reads(&lib), second);
}

#[test]
fn a_book_with_a_broken_cover_is_not_tried_again() {
    let mut lib = library(vec![("A.EPU", book(Some("not_an_image.txt"), true))]);
    draw_list(&mut lib, 0);
    let before = lib.storage().reads.borrow().clone();
    draw_list(&mut lib, 0);
    assert_eq!(*lib.storage().reads.borrow(), before);
}

#[test]
fn the_real_books_show_their_covers() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/books");
    let names = ["alice.epub", "gulliver.epub", "journey.epub"];
    let mut lib = library(
        names
            .iter()
            .map(|n| (*n, std::fs::read(format!("{dir}/{n}")).unwrap()))
            .collect(),
    );
    let canvas = draw_list(&mut lib, 1);
    if let Some(dir) = support::dump_dir() {
        std::fs::create_dir_all(&dir).unwrap();
        let mut file = format!("P5\n{} {}\n255\n", G.width, G.height).into_bytes();
        file.extend(canvas.pixels.iter().map(|&p| p * 17));
        std::fs::write(format!("{dir}/list_real_covers.pgm"), file).unwrap();
    }
    let (width, height) = cover_size(&G);
    let x = G.margin_left + 20;
    // The cover is not blank when it has several gray levels.
    let levels = |slot: u32| {
        let y = row(slot) + 20;
        let mut levels = std::collections::BTreeSet::new();
        for py in y..y + height {
            for px in x..x + width {
                levels.insert(canvas.at(px, py));
            }
        }
        levels.len()
    };
    assert!(levels(0) > 3, "alice: cover has {} levels", levels(0));
    assert!(levels(1) > 3, "gulliver: cover has {} levels", levels(1));
    // The cover of "A Journey" is an 8 bit RGB PNG of 1600 by 2400, which only a row at a time fits.
    assert!(levels(2) > 3, "journey: cover has {} levels", levels(2));
}
