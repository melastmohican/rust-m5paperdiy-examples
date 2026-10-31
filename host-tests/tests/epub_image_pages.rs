//! Tests of the pictures in the pages: the library reads the size of each picture from the book, so it
//! takes room on its page, and draws it where the layout put it. The books are made in the test with
//! the pictures of `fixtures/images/` (40 by 20, the left half black, the right half white, and a
//! 1000 by 500 one), and the card is a [`Storage`] in memory.

mod support;

use embedded_graphics::{Pixel, pixelcolor::Gray4, prelude::*};
use m5paperdiy_common::reader::{Action, App, Effect};
use m5paperdiy_common::render::Geometry;
use m5paperdiy_common::sd::{BookEntry, BookLibrary, PictureStatus};
use m5paperdiy_common::storage::{DirEntry, Storage, StorageError};
use std::collections::BTreeMap;

const G: Geometry = Geometry::PORTRAIT;

/// Files in memory.
#[derive(Default)]
struct Mem(BTreeMap<String, Vec<u8>>);

impl Storage for Mem {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let data = self.0.get(path).ok_or(StorageError::NotFound)?;
        let start = (offset as usize).min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0
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

/// A book of one document, `body`, with the pictures `(name, file)` at `OEBPS/images/name`.
fn book(body: &str, pictures: &[(&str, &str)]) -> Vec<u8> {
    const CONTAINER: &str = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    let opf = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Pictures</dc:title></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c" href="ch.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c"/></spine></package>"#;
    let ncx = r#"<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint id="n"><navLabel><text>All</text></navLabel><content src="ch.xhtml"/></navPoint></navMap></ncx>"#;
    let page = format!("<html><body>{body}</body></html>");
    let pictures: Vec<(String, Vec<u8>)> = pictures
        .iter()
        .map(|(name, file)| (format!("OEBPS/images/{name}"), fixture(file)))
        .collect();
    let mut files: Vec<(&str, &[u8])> = vec![
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/toc.ncx", ncx.as_bytes()),
        ("OEBPS/ch.xhtml", page.as_bytes()),
    ];
    for (name, data) in &pictures {
        files.push((name, data));
    }
    support::zip_bytes(&files, &["OEBPS/ch.xhtml"])
}

fn library(book: Vec<u8>) -> BookLibrary<Mem> {
    let mut mem = Mem::default();
    mem.0.insert("book.epub".to_string(), book);
    BookLibrary::with_books(
        mem,
        vec![BookEntry {
            path: "book.epub".to_string(),
            title: "Pictures".to_string(),
        }],
        G,
    )
}

/// The screen in memory, one level 0 (black) to 15 (white) a pixel.
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

    /// The first and the last row that has a pixel darker than 8 within the columns `from..to`.
    fn dark_rows(&self, from: u32, to: u32) -> Option<(u32, u32)> {
        let rows: Vec<u32> = (0..G.height)
            .filter(|&y| (from..to).any(|x| self.at(x, y) < 8))
            .collect();
        Some((*rows.first()?, *rows.last()?))
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

/// Opens the book and draws its first page.
fn first_page(lib: &mut BookLibrary<Mem>) -> Canvas {
    let mut app = App::new();
    assert_eq!(app.press(Action::Select, lib), Effect::Redraw); // contents
    assert_eq!(app.press(Action::Select, lib), Effect::Redraw); // the page
    let mut canvas = Canvas::white();
    lib.draw(&app, Effect::Redraw, &mut canvas);
    canvas
}

/// Where the left edge of a picture `width` wide is on the screen, and the row under the first line.
fn picture_left(width: u32) -> u32 {
    G.margin_left + (G.page_width() - width) / 2
}

const BEFORE_AFTER: &str = r#"<p>BEFORE</p><img src="images/a.png"/><p>AFTER</p>"#;

#[test]
fn a_picture_is_drawn_between_the_paragraphs_around_it() {
    let mut lib = library(book(BEFORE_AFTER, &[("a.png", "gray.png")]));
    let canvas = first_page(&mut lib);
    // One line and the half line under it come first.
    let line = lib.line_height();
    let top = G.margin_top + line + line / 2;
    let left = picture_left(40);
    for y in top..top + 20 {
        for x in left..left + 20 {
            assert_eq!(canvas.at(x, y), 0, "black half at {x},{y}");
        }
        for x in left + 20..left + 40 {
            assert_eq!(canvas.at(x, y), 15, "white half at {x},{y}");
        }
    }
    assert_eq!(canvas.at(left - 1, top + 5), 15);
    assert_eq!(canvas.at(left + 5, top - 1), 15);
    assert_eq!(canvas.at(left + 5, top + 20), 15);
    // The text is at the left, so its columns do not cross the picture's: BEFORE above it and AFTER
    // under it, the first dark row of AFTER below the picture.
    let (first, last) = canvas.dark_rows(0, left).expect("the text has ink");
    assert!(first < top, "BEFORE starts above the picture");
    assert!(last >= top + 20, "AFTER ends under the picture: {last}");
}

#[test]
fn a_picture_pushes_the_text_after_it_down() {
    let with = first_page(&mut library(book(BEFORE_AFTER, &[("a.png", "gray.png")])));
    let without = first_page(&mut library(book(r#"<p>BEFORE</p><p>AFTER</p>"#, &[])));
    let (_, last_with) = with.dark_rows(0, 200).unwrap();
    let (_, last_without) = without.dark_rows(0, 200).unwrap();
    assert_eq!(last_with - last_without, 20, "the picture is 20 rows high");
}

#[test]
fn a_picture_that_is_not_in_the_book_leaves_no_gap() {
    let missing = first_page(&mut library(book(
        r#"<p>BEFORE</p><img src="images/gone.png"/><p>AFTER</p>"#,
        &[],
    )));
    let none = first_page(&mut library(book(r#"<p>BEFORE</p><p>AFTER</p>"#, &[])));
    assert_eq!(missing.pixels, none.pixels);
}

#[test]
fn a_picture_that_is_not_a_picture_leaves_no_gap() {
    // The file is there but is text, not a PNG: it has no size, so nothing is reserved or drawn.
    let mut lib = library(book(BEFORE_AFTER, &[("a.png", "not_an_image.txt")]));
    let broken = first_page(&mut lib);
    let none = first_page(&mut library(book(r#"<p>BEFORE</p><p>AFTER</p>"#, &[])));
    assert_eq!(broken.pixels, none.pixels);
}

#[test]
fn a_picture_that_cannot_be_decoded_keeps_its_room_empty() {
    // The header of a cut-off PNG is whole, so its size is known and it takes room, but there is
    // nothing to draw in it.
    let broken = first_page(&mut library(book(
        BEFORE_AFTER,
        &[("a.png", "truncated.png")],
    )));
    let whole = first_page(&mut library(book(BEFORE_AFTER, &[("a.png", "gray.png")])));
    let left = picture_left(40);
    let (_, last_broken) = broken.dark_rows(0, left).unwrap();
    let (_, last_whole) = whole.dark_rows(0, left).unwrap();
    assert_eq!(
        last_broken, last_whole,
        "the text after it is where it would be"
    );
    for y in 0..G.height {
        for x in left..left + 40 {
            assert_eq!(broken.at(x, y), 15, "empty at {x},{y}");
        }
    }
}

#[test]
fn a_wide_picture_is_scaled_to_the_page() {
    // 1000 by 500 becomes 520 by 260 on a page 520 wide; the left half is black.
    let mut lib = library(book(
        r#"<img src="images/w.png"/>"#,
        &[("w.png", "wide.png")],
    ));
    let canvas = first_page(&mut lib);
    let top = G.margin_top;
    let left = G.margin_left;
    assert_eq!(canvas.at(left, top), 0);
    assert_eq!(canvas.at(left + 259, top + 259), 0);
    assert_eq!(canvas.at(left + 260, top), 15);
    assert_eq!(canvas.at(left + 519, top + 259), 15);
    assert_eq!(canvas.dark_rows(0, G.width), Some((top, top + 259)));
}

#[test]
fn a_jpeg_is_drawn_too() {
    let mut lib = library(book(
        r#"<img src="images/c.jpg"/>"#,
        &[("c.jpg", "color.jpg")],
    ));
    let canvas = first_page(&mut lib);
    let left = picture_left(40);
    let top = G.margin_top;
    // Red is about level 4 or 5 of 15, blue about level 1 or 2.
    for (x, wanted) in [(left + 5, 3..=6), (left + 35, 0..=3)] {
        let level = canvas.at(x, top + 10);
        assert!(wanted.contains(&level), "level {level} at {x}");
    }
}

#[test]
fn the_library_says_what_became_of_each_picture_of_the_last_page() {
    let mut lib = library(book(BEFORE_AFTER, &[("a.png", "gray.png")]));
    let mut app = App::new();
    app.press(Action::Select, &mut lib);
    app.press(Action::Select, &mut lib);
    let mut canvas = Canvas::white();
    lib.draw(&app, Effect::Redraw, &mut canvas);
    let size = fixture("gray.png").len();
    assert_eq!(
        lib.last_pictures(),
        [(
            "OEBPS/images/a.png".to_string(),
            PictureStatus::Drawn { bytes: size }
        )]
    );
    // Back on the list there is no page, so no picture.
    app.press(Action::Select, &mut lib);
    lib.draw(&app, Effect::Redraw, &mut canvas);
    assert!(lib.last_pictures().is_empty());

    let mut lib = library(book(BEFORE_AFTER, &[("a.png", "truncated.png")]));
    let mut app = App::new();
    app.press(Action::Select, &mut lib);
    app.press(Action::Select, &mut lib);
    lib.draw(&app, Effect::Redraw, &mut canvas);
    let size = fixture("truncated.png").len();
    assert_eq!(
        lib.last_pictures(),
        [(
            "OEBPS/images/a.png".to_string(),
            PictureStatus::NotDecoded { bytes: size }
        )]
    );
}

#[test]
fn the_second_page_of_a_chapter_with_a_picture_draws_too() {
    // Enough text to push the picture to a later page: it is drawn there and not on the first.
    let text: String = (0..60).map(|n| format!("<p>Paragraph {n}</p>")).collect();
    let body = format!(r#"{text}<img src="images/a.png"/>"#);
    let mut lib = library(book(&body, &[("a.png", "gray.png")]));
    let mut app = App::new();
    app.press(Action::Select, &mut lib);
    app.press(Action::Select, &mut lib);
    let left = picture_left(40);
    let mut seen_on = Vec::new();
    for page in 0..8 {
        let mut canvas = Canvas::white();
        lib.draw(&app, Effect::Redraw, &mut canvas);
        if canvas.dark_rows(left, left + 20).is_some() {
            seen_on.push(page);
        }
        if app.press(Action::Down, &mut lib) != Effect::Redraw {
            break;
        }
    }
    assert_eq!(
        seen_on.len(),
        1,
        "the picture is on exactly one page: {seen_on:?}"
    );
    assert!(seen_on[0] > 0, "and not the first");
}

/// Files on disk, by path.
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

/// Real books, from `EPUB_TEST_FILES`: every page of every book is drawn, pictures included. Nothing
/// may panic, and the time per page is printed.
#[test]
#[ignore = "needs real books: set EPUB_TEST_FILES"]
fn every_page_of_real_books_draws() {
    let files = std::env::var("EPUB_TEST_FILES").expect("set EPUB_TEST_FILES");
    for path in files.split(':').filter(|p| !p.is_empty()) {
        let mut lib = BookLibrary::with_books(
            Disk,
            vec![BookEntry {
                path: path.to_string(),
                title: "Book".to_string(),
            }],
            G,
        );
        let mut canvas = Canvas::white();
        let mut app = App::new();
        lib.press(&mut app, Action::Select, &mut canvas);
        lib.press(&mut app, Action::Select, &mut canvas);
        let started = std::time::Instant::now();
        let mut pages = 0u32;
        let mut slowest = std::time::Duration::ZERO;
        loop {
            let one = std::time::Instant::now();
            let effect = lib.press(&mut app, Action::Down, &mut canvas);
            slowest = slowest.max(one.elapsed());
            if effect != Effect::Redraw || pages >= 3000 {
                break;
            }
            for picture in lib.pictures_on_page(&app) {
                let entry = lib
                    .toc_entries()
                    .iter()
                    .position(|e| e.spine_index == Some(app.section()));
                println!(
                    "   picture on document {} page {} (contents entry {:?} {:?}): {picture}",
                    app.section(),
                    app.page(),
                    entry,
                    entry.map(|i| lib.toc_entries()[i].title.clone())
                );
                if let Some(dir) = support::dump_dir() {
                    let name = format!("picture_d{}_p{}", app.section(), app.page());
                    std::fs::create_dir_all(&dir).unwrap();
                    let mut file = format!("P5\n{} {}\n255\n", G.width, G.height).into_bytes();
                    file.extend(canvas.pixels.iter().map(|&p| p * 17));
                    std::fs::write(format!("{dir}/{name}.pgm"), file).unwrap();
                }
            }
            pages += 1;
        }
        println!(
            "{path}: {pages} pages drawn in {:?}, slowest {:?}",
            started.elapsed(),
            slowest
        );
    }
}

/// The books in `fixtures/books/`, opened from their files. Counts the presses of A needed to reach
/// the pages with a picture, which are the same as on the board.
fn repo_book(name: &str) -> (BookLibrary<Disk>, App) {
    let path = format!("{}/tests/fixtures/books/{name}", env!("CARGO_MANIFEST_DIR"));
    let lib = BookLibrary::with_books(
        Disk,
        vec![BookEntry {
            path,
            title: "Book".to_string(),
        }],
        G,
    );
    (lib, App::new())
}

fn press_n(lib: &mut BookLibrary<Disk>, app: &mut App, action: Action, times: usize) {
    let mut canvas = Canvas::white();
    for _ in 0..times {
        assert_eq!(lib.press(app, action, &mut canvas), Effect::Redraw);
    }
}

#[test]
fn a_png_in_a_real_book_is_drawn_on_its_page() {
    // "A Journey": the contents, the book from its first entry, then 13 pages on: the runic parchment.
    let (mut lib, mut app) = repo_book("journey.epub");
    press_n(&mut lib, &mut app, Action::Select, 2);
    press_n(&mut lib, &mut app, Action::Down, 13);
    let mut canvas = Canvas::white();
    lib.draw(&app, Effect::Redraw, &mut canvas);
    assert_eq!(
        lib.last_pictures(),
        [(
            "OEBPS/4844069071107539737_image01.png".to_string(),
            PictureStatus::Drawn { bytes: 4120 }
        )]
    );
    // The parchment is narrower than the text: its rows have ink in the middle and none at the left
    // margin, where every line of text starts. The heading above it is centred too, and is a line high.
    let rows = (0..G.height)
        .filter(|&y| {
            (200..300).any(|x| canvas.at(x, y) < 8) && (10..40).all(|x| canvas.at(x, y) >= 8)
        })
        .count();
    assert!(rows > 100, "{rows} rows of picture");
}

#[test]
fn a_jpeg_in_a_real_book_is_drawn_on_its_page() {
    // "Gulliver's", contents entry 27 (III.V), 7 pages on: the word frame of Lagado.
    let (mut lib, mut app) = repo_book("gulliver.epub");
    press_n(&mut lib, &mut app, Action::Select, 1);
    press_n(&mut lib, &mut app, Action::Down, 27);
    press_n(&mut lib, &mut app, Action::Select, 1);
    press_n(&mut lib, &mut app, Action::Down, 7);
    let mut canvas = Canvas::white();
    lib.draw(&app, Effect::Redraw, &mut canvas);
    assert_eq!(
        lib.last_pictures(),
        [(
            "OEBPS/5439318815537153844_theframe.jpg".to_string(),
            PictureStatus::Drawn { bytes: 227325 }
        )]
    );
    // The frame is 500 by 496 on a page 520 wide: its middle is inked and the grid has grays.
    let levels: std::collections::BTreeSet<u8> = canvas.pixels.iter().copied().collect();
    assert!(levels.len() >= 8, "{} gray levels", levels.len());
    let ink = canvas.pixels.iter().filter(|&&p| p < 8).count();
    assert!(ink > 20000, "{ink}");
}
