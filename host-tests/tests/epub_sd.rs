//! Tests of `common/src/sd.rs`: reading files and books from a FAT card, and the reader running on a
//! card, with the card in memory. Long file names cannot be written with `embedded-sdmmc`, so the
//! scan for `.epub` files is checked by the name test here and, on a real FAT image with long names,
//! by the ignored test at the end.

mod support;

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use embedded_sdmmc::{
    Block, BlockCount, BlockDevice, BlockIdx, Mode, TimeSource, Timestamp, VolumeIdx, VolumeManager,
};
use m5paperdiy_common::epub::Book;
use m5paperdiy_common::reader::{Action, App, Effect, Library, Screen};
use m5paperdiy_common::render::Geometry;
use m5paperdiy_common::sd::{BookEntry, SdLibrary, SdStorage, is_epub_name};
use m5paperdiy_common::storage::{Storage, StorageError};
use std::cell::RefCell;
use std::rc::Rc;

use Action::{Down, Select, Up};

const G: Geometry = Geometry::PORTRAIT;

// ---------------------------------------------------------------------------------------------
// A card in memory.

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
            zero_indexed_day: 30,
            hours: 12,
            minutes: 0,
            seconds: 0,
        }
    }
}

/// A card with these files in the root, by short name.
fn card(files: &[(&str, &[u8])]) -> Ram {
    let ram = Ram(Rc::new(RefCell::new(support::fat32_image())));
    let mgr = VolumeManager::new(ram.clone(), Time);
    let volume = mgr.open_raw_volume(VolumeIdx(0)).unwrap();
    let root = mgr.open_root_dir(volume).unwrap();
    for (name, data) in files {
        let file = mgr
            .open_file_in_dir(root, *name, Mode::ReadWriteCreate)
            .unwrap();
        mgr.write(file, data).unwrap();
        mgr.close_file(file).unwrap();
    }
    mgr.close_dir(root).unwrap();
    mgr.close_volume(volume).unwrap();
    ram
}

/// Gives the files of `ram`, made with `card` under short names, their long names. The root
/// directory is written again as a Mac or Windows writes it: for each `(short name, long name, hidden)`
/// the long name entries, last part first, then the short name entry the library made. `embedded-sdmmc`
/// cannot write long names itself.
fn give_long_names(ram: &Ram, files: &[(&str, &str, bool)]) {
    // The first root directory sector: partition at 2048, 32 reserved, two FATs of 624 sectors.
    const ROOT: usize = (2048 + 32 + 2 * 624) * 512;
    let mut image = ram.0.borrow_mut();
    let old: Vec<[u8; 32]> = image[ROOT..ROOT + 512]
        .chunks_exact(32)
        .map(|entry| entry.try_into().unwrap())
        .collect();
    let mut out = Vec::new();
    for (short, long, hidden) in files {
        let (name, ext) = short.split_once('.').unwrap();
        let mut raw = [b' '; 11];
        raw[..name.len()].copy_from_slice(name.as_bytes());
        raw[8..8 + ext.len()].copy_from_slice(ext.as_bytes());
        let mut entry = *old
            .iter()
            .find(|e| e[..11] == raw)
            .unwrap_or_else(|| panic!("{short} is not in the directory"));
        if *hidden {
            entry[11] |= 0x02;
        }
        let csum = raw
            .iter()
            .fold(0u8, |sum, b| sum.rotate_right(1).wrapping_add(*b));
        let mut units: Vec<u16> = long.encode_utf16().collect();
        if units.len() % 13 != 0 {
            units.push(0);
        }
        while units.len() % 13 != 0 {
            units.push(0xFFFF);
        }
        let parts = units.len() / 13;
        for part in (0..parts).rev() {
            let mut lfn = [0u8; 32];
            lfn[0] = part as u8 + 1 + if part == parts - 1 { 0x40 } else { 0 };
            lfn[11] = 0x0F;
            lfn[13] = csum;
            let chunk = &units[part * 13..part * 13 + 13];
            for (i, unit) in chunk.iter().enumerate() {
                let at = match i {
                    0..=4 => 1 + i * 2,
                    5..=10 => 14 + (i - 5) * 2,
                    _ => 28 + (i - 11) * 2,
                };
                lfn[at..at + 2].copy_from_slice(&unit.to_le_bytes());
            }
            out.push(lfn);
        }
        out.push(entry);
    }
    assert!(out.len() <= 16, "the directory is one sector");
    image[ROOT..ROOT + 512].fill(0);
    for (i, entry) in out.iter().enumerate() {
        image[ROOT + i * 32..ROOT + i * 32 + 32].copy_from_slice(entry);
    }
}

fn fixture_book(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/books/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The screen in memory, 0 black to 15 white.
struct Canvas {
    pixels: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        Self {
            pixels: vec![15; (G.width * G.height) as usize],
        }
    }
    fn ink(&self, rows: std::ops::Range<usize>) -> usize {
        rows.map(|y| {
            self.pixels[y * G.width as usize..(y + 1) * G.width as usize]
                .iter()
                .filter(|&&p| p < 8)
                .count()
        })
        .sum()
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
        for Pixel(p, c) in pixels {
            if p.x >= 0 && p.y >= 0 && (p.x as u32) < G.width && (p.y as u32) < G.height {
                self.pixels[(p.y as u32 * G.width + p.x as u32) as usize] = c.luma();
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------

#[test]
fn only_epub_names_are_books() {
    for name in [
        "Book.epub",
        "BOOK.EPUB",
        "a.EpUb",
        "My Book (2nd ed).epub",
        "x.y.epub",
    ] {
        assert!(is_epub_name(name), "{name}");
    }
    for name in [
        "x.epub.txt",
        ".epub",
        "epub",
        "book.epu",
        "._Book.epub",
        ".hidden.epub",
        "",
        "book.pdf",
    ] {
        assert!(!is_epub_name(name), "{name}");
    }
}

#[test]
fn a_file_is_read_in_pieces_from_any_offset() {
    let data: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
    let storage = SdStorage::new(card(&[("DATA.BIN", &data)]), Time)
        .ok()
        .unwrap();
    assert_eq!(storage.size("DATA.BIN"), Ok(5000));
    // Pieces across block (512) and cluster boundaries, one past the end, and far past it.
    for (offset, len) in [
        (0, 10),
        (505, 20),
        (511, 2),
        (1000, 3000),
        (4990, 100),
        (4999, 1),
    ] {
        let mut buf = vec![0u8; len];
        let n = storage.read_at("DATA.BIN", offset, &mut buf).unwrap();
        let want = &data[offset as usize..(offset as usize + len).min(5000)];
        assert_eq!(&buf[..n], want, "offset {offset}, length {len}");
        assert_eq!(n, want.len());
    }
    let mut buf = [0u8; 8];
    assert_eq!(storage.read_at("DATA.BIN", 5000, &mut buf), Ok(0));
    assert_eq!(storage.read_at("DATA.BIN", 9000, &mut buf), Ok(0));
    assert_eq!(
        storage.read_at("MISSING.BIN", 0, &mut buf),
        Err(StorageError::NotFound)
    );
    assert_eq!(storage.size("MISSING.BIN"), Err(StorageError::NotFound));
    // The card is read only.
    assert_eq!(
        storage.write("DATA.BIN", b"x"),
        Err(StorageError::Unsupported)
    );
    assert_eq!(
        storage.append("DATA.BIN", b"x"),
        Err(StorageError::Unsupported)
    );
    assert_eq!(storage.remove("DATA.BIN"), Err(StorageError::Unsupported));
}

#[test]
fn reading_two_files_by_turns_keeps_working() {
    let a = vec![1u8; 3000];
    let b = vec![2u8; 3000];
    let storage = SdStorage::new(card(&[("A.BIN", &a), ("B.BIN", &b)]), Time)
        .ok()
        .unwrap();
    let mut buf = [0u8; 100];
    // Far more switches than the card can hold files open.
    for round in 0..50 {
        storage.read_at("A.BIN", round * 10, &mut buf).unwrap();
        assert!(buf.iter().all(|&x| x == 1), "round {round}");
        storage.read_at("B.BIN", round * 10, &mut buf).unwrap();
        assert!(buf.iter().all(|&x| x == 2), "round {round}");
    }
}

#[test]
fn a_book_opens_from_the_card_and_its_documents_read() {
    let epub = support::simple_epub(
        "On the Card",
        &[("One", "<p>first</p>"), ("Two", "<p>second</p>")],
    );
    let storage = SdStorage::new(card(&[("BOOK.EPU", &epub)]), Time)
        .ok()
        .unwrap();
    let book = Book::open(&storage, "BOOK.EPU").unwrap();
    assert_eq!(book.title(), "On the Card");
    assert_eq!(book.toc().len(), 2);
    assert_eq!(
        book.read_spine(&storage, 1).unwrap(),
        b"<html><body><p>second</p></body></html>"
    );
    // Nothing on the card is an EPUB by long name, so the scan finds none, and the library says so.
    let library = SdLibrary::new(storage, G).ok().unwrap();
    assert!(library.books().is_empty());
}

fn text_chapter(words: usize) -> String {
    let body: Vec<&str> = (0..words).map(|_| "reading").collect();
    format!("<h1>Heading</h1><p>{}</p>", body.join(" "))
}

fn library_with(books: &[(&str, &str, Vec<(&str, String)>)]) -> SdLibrary<Ram, Time> {
    let files: Vec<(String, Vec<u8>)> = books
        .iter()
        .map(|(file, title, chapters)| {
            let chapters: Vec<(&str, &str)> =
                chapters.iter().map(|(n, b)| (*n, b.as_str())).collect();
            (file.to_string(), support::simple_epub(title, &chapters))
        })
        .collect();
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    let storage = SdStorage::new(card(&refs), Time).ok().unwrap();
    let entries = books
        .iter()
        .map(|(file, title, _)| BookEntry {
            path: file.to_string(),
            title: title.to_string(),
        })
        .collect();
    SdLibrary::with_books(storage, entries, G)
}

#[test]
fn the_reader_runs_on_the_card_from_the_list_to_the_last_page_and_back() {
    let mut lib = library_with(&[
        (
            "ALPHA.EPU",
            "Alpha",
            vec![("A one", text_chapter(300)), ("A two", text_chapter(40))],
        ),
        ("BETA.EPU", "Beta", vec![("B one", text_chapter(10))]),
    ]);
    let mut app = App::new();
    let mut screen = Canvas::new();

    // The list.
    lib.draw(&app, Effect::Redraw, &mut screen);
    assert!(screen.ink(35..960) > 0, "the list shows the books");

    // Down to Beta and back up, then open Alpha: its table of contents has two entries.
    assert_eq!(
        lib.press(&mut app, Down, &mut Canvas::new()),
        Effect::Redraw
    );
    assert_eq!(app.selected_book(), 1);
    lib.press(&mut app, Up, &mut Canvas::new());
    let mut screen = Canvas::new();
    assert_eq!(lib.press(&mut app, Select, &mut screen), Effect::Redraw);
    assert_eq!(app.screen(), Screen::Contents);
    assert_eq!(lib.contents().len(), 2);
    assert!(screen.ink(35..960) > 0, "the contents are drawn");

    // Read the first chapter from the start.
    let mut screen = Canvas::new();
    assert_eq!(lib.press(&mut app, Select, &mut screen), Effect::Redraw);
    assert_eq!(
        (app.screen(), app.section(), app.page()),
        (Screen::Reading, 0, 0)
    );
    assert!(
        screen.ink(35..200) > 0,
        "the first page is drawn from the top"
    );

    // Forward through every page of the book, drawing each, to the end.
    let mut pages = vec![(app.section(), app.page())];
    loop {
        let mut screen = Canvas::new();
        match lib.press(&mut app, Down, &mut screen) {
            Effect::Redraw => {
                pages.push((app.section(), app.page()));
                assert!(screen.ink(35..960) > 0, "page {:?} is drawn", pages.last());
            }
            Effect::EndOfBook => {
                // The notice is drawn over the last page, in the middle of the screen.
                assert!(screen.ink(400..560) > 0, "the end of the book notice");
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(pages.len() >= 3, "300 words take several pages: {pages:?}");
    assert_eq!(pages.first(), Some(&(0, 0)));
    assert_eq!(
        pages.last().map(|p| p.0),
        Some(1),
        "ends in the second chapter"
    );

    // And back to the start of the book.
    let mut back = vec![(app.section(), app.page())];
    loop {
        let mut screen = Canvas::new();
        match lib.press(&mut app, Up, &mut screen) {
            Effect::Redraw => back.push((app.section(), app.page())),
            Effect::StartOfBook => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    back.reverse();
    assert_eq!(back, pages, "going back retraces the pages");

    // Select returns to the list, with the same book chosen.
    let mut screen = Canvas::new();
    assert_eq!(lib.press(&mut app, Select, &mut screen), Effect::Redraw);
    assert_eq!((app.screen(), app.selected_book()), (Screen::List, 0));
    // The other book opens too.
    lib.press(&mut app, Down, &mut Canvas::new());
    assert_eq!(
        lib.press(&mut app, Select, &mut Canvas::new()),
        Effect::Redraw
    );
    assert_eq!(lib.contents().len(), 1);
}

#[test]
fn a_book_that_cannot_be_read_shows_a_notice_and_stays_on_the_list() {
    let storage = SdStorage::new(card(&[("JUNK.EPU", b"not a zip at all")]), Time)
        .ok()
        .unwrap();
    let mut lib = SdLibrary::with_books(
        storage,
        vec![BookEntry {
            path: "JUNK.EPU".into(),
            title: "Junk".into(),
        }],
        G,
    );
    let mut app = App::new();
    let mut screen = Canvas::new();
    assert_eq!(lib.press(&mut app, Select, &mut screen), Effect::OpenFailed);
    assert_eq!(app.screen(), Screen::List);
    assert!(
        screen.ink(400..560) > 0,
        "the notice is drawn in the middle"
    );
    assert_eq!(lib.book_count(), 1);
}

#[test]
fn a_card_without_books_draws_a_notice_and_does_nothing_on_a_press() {
    let storage = SdStorage::new(card(&[("NOTES.TXT", b"hello")]), Time)
        .ok()
        .unwrap();
    let mut lib = SdLibrary::new(storage, G).ok().unwrap();
    let mut app = App::new();
    let mut screen = Canvas::new();
    lib.draw(&app, Effect::Redraw, &mut screen);
    assert!(screen.ink(400..560) > 0, "No books on the card");
    assert_eq!(
        lib.press(&mut app, Select, &mut Canvas::new()),
        Effect::None
    );
    assert_eq!(app.screen(), Screen::List);
}

/// A real FAT image with long file names, from `EPUB_FAT_IMAGE`: the scan finds the `.epub` files by
/// their long name, leaves out the `._` files and the other kinds, and the library lists the books by
/// title.
#[test]
#[ignore = "needs a FAT image with .epub files: set EPUB_FAT_IMAGE"]
fn a_real_card_image_lists_its_books_by_long_name() {
    let path = std::env::var("EPUB_FAT_IMAGE").expect("set EPUB_FAT_IMAGE");
    let image = std::fs::read(&path).unwrap();
    let ram = Ram(Rc::new(RefCell::new(image)));
    let storage = SdStorage::new(ram, Time).ok().expect("a FAT volume");
    let names = storage.epub_files().ok().unwrap();
    println!("epub files: {names:?}");
    assert!(!names.is_empty(), "no .epub file found by long name");
    assert!(names.iter().all(|n| is_epub_name(n)));
    let library = SdLibrary::new(storage, G).ok().unwrap();
    assert_eq!(
        library.books().len(),
        names.len(),
        "a book was found by the scan and then could not be opened"
    );
    for book in library.books() {
        println!("book: {:?} ({})", book.title, book.path);
        assert!(!book.title.is_empty());
    }
    let titles: Vec<&str> = library.books().iter().map(|b| b.title.as_str()).collect();
    let mut sorted = titles.clone();
    sorted.sort();
    assert_eq!(titles, sorted, "sorted by title");

    // Read every book from its first chapter to its last page and back, drawing each page.
    let mut library = library;
    for index in 0..library.books().len() {
        let title = library.books()[index].title.clone();
        let mut app = App::new();
        while app.selected_book() != index {
            library.press(&mut app, Down, &mut Canvas::new());
        }
        let started = std::time::Instant::now();
        assert_eq!(
            library.press(&mut app, Select, &mut Canvas::new()),
            Effect::Redraw
        );
        assert_eq!(
            library.press(&mut app, Select, &mut Canvas::new()),
            Effect::Redraw
        );
        let (mut forward, mut canvas) = (1usize, Canvas::new());
        while library.press(&mut app, Down, &mut canvas) == Effect::Redraw {
            forward += 1;
        }
        let mut back = 1usize;
        while library.press(&mut app, Up, &mut Canvas::new()) == Effect::Redraw {
            back += 1;
        }
        println!(
            "read {title:?}: {forward} pages forward, {back} back, {:?}",
            started.elapsed()
        );
        assert!(forward > 1 && back >= forward);
    }
}

/// `embedded-sdmmc` 0.10.0 finds a file by its long name with a matcher that gets stuck when an
/// earlier name looks like the end of the one searched for (every `._name.epub` a Mac leaves ends
/// in "b"): the files after it are then "not found". The card of the user lost "journey.epub" so,
/// the image of a Mac lost "gulliver.epub". Books are opened by the short name the scan found.
#[test]
fn every_book_opens_whatever_long_names_come_before_it() {
    let (journey, gulliver, alice) = (
        fixture_book("journey.epub"),
        fixture_book("gulliver.epub"),
        fixture_book("alice.epub"),
    );
    let junk = [0u8; 16];
    let ram = card(&[
        ("JOURNE~1.EPU", &journey),
        ("_JOURN~3.EPU", &junk),
        ("GULLIV~4.EPU", &gulliver),
        ("_GULLI~6.EPU", &junk),
        ("ALIC~7.EPU", &alice),
        ("_ALIC~9.EPU", &junk),
    ]);
    give_long_names(
        &ram,
        &[
            ("JOURNE~1.EPU", "journey.epub", false),
            ("_JOURN~3.EPU", "._journey.epub", true),
            ("GULLIV~4.EPU", "gulliver.epub", false),
            ("_GULLI~6.EPU", "._gulliver.epub", true),
            ("ALIC~7.EPU", "alice.epub", false),
            ("_ALIC~9.EPU", "._alice.epub", true),
        ],
    );
    let storage = SdStorage::new(ram, Time).ok().expect("a FAT volume");
    assert_eq!(
        storage.epub_files().ok().unwrap(),
        ["journey.epub", "gulliver.epub", "alice.epub"]
    );
    let library = SdLibrary::new(storage, G).ok().unwrap();
    let titles: Vec<&str> = library.books().iter().map(|b| b.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "A Journey to the Centre of the Earth",
            "Alice's Adventures in Wonderland",
            "Gulliver's Travels into Several Remote Nations of the World"
        ]
    );
}
