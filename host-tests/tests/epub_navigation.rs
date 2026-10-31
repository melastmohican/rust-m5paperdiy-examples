//! Tests of how the reader navigates: choosing an entry of the table of contents must land on the page
//! where that chapter starts. The books are made in the test, in the two shapes real books have, and the
//! real books named in `EPUB_TEST_FILES` are read from their files. The card is a [`Storage`] in memory
//! or a file, so no FAT image is needed.

mod support;

use m5paperdiy_common::reader::{Action, App, Effect, Screen};
use m5paperdiy_common::render::Geometry;
use m5paperdiy_common::sd::{BookEntry, BookLibrary};
use m5paperdiy_common::storage::{DirEntry, Storage, StorageError};
use std::collections::BTreeMap;

use Action::{Down, Select, Up};

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

fn library<S: Storage>(storage: S, path: &str) -> BookLibrary<S> {
    BookLibrary::with_books(
        storage,
        vec![BookEntry {
            path: path.to_string(),
            title: "Book".to_string(),
        }],
        G,
    )
}

// ---------------------------------------------------------------------------------------------
// Two shapes of book.

const CONTAINER: &str = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;

/// A paragraph of `words` words, each marked with the chapter and its number.
fn prose(chapter: usize, words: usize) -> String {
    let words: Vec<String> = (0..words).map(|k| format!("c{chapter}w{k}")).collect();
    format!("<p>{}</p>", words.join(" "))
}

/// A book the way Project Gutenberg makes them: a cover file, then ONE big document holding the whole
/// text, with every chapter heading carrying an `id`, and a table of contents of `file#id` links, the
/// chapter headings at level 0 and their subtitles at level 1. "A Journey to the Centre of the Earth"
/// on the author's card is exactly this.
fn gutenberg_epub(chapters: usize, words_per_chapter: usize) -> Vec<u8> {
    let mut book = String::from(
        r#"<h2 id="pgepubid00000">THE BOOK</h2><h2 id="pgepubid00001">Contents</h2><p>"#,
    );
    book.push_str(
        &(1..=chapters)
            .map(|n| format!("CHAPTER {n}"))
            .collect::<Vec<_>>()
            .join(" "),
    );
    book.push_str("</p>");
    let mut ncx = String::from(
        r##"<navPoint id="n0"><navLabel><text>THE BOOK</text></navLabel><content src="book.xhtml#pgepubid00000"/></navPoint><navPoint id="n1"><navLabel><text>Contents</text></navLabel><content src="book.xhtml#pgepubid00001"/></navPoint>"##,
    );
    for n in 1..=chapters {
        let id = 2 + (n - 1) * 2;
        book.push_str(&format!(
            r#"<h2 id="pgepubid{id:05}">CHAPTER {n}</h2><h3 id="pgepubid{:05}">SUBTITLE {n}</h3>"#,
            id + 1
        ));
        book.push_str(&prose(n, words_per_chapter));
        ncx.push_str(&format!(
            r##"<navPoint id="c{n}"><navLabel><text>CHAPTER {n}</text></navLabel><content src="book.xhtml#pgepubid{id:05}"/><navPoint id="s{n}"><navLabel><text>SUBTITLE {n}</text></navLabel><content src="book.xhtml#pgepubid{:05}"/></navPoint></navPoint>"##,
            id + 1
        ));
    }
    let opf = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Gutenberg style</dc:title></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="cover" href="cover.xhtml" media-type="application/xhtml+xml"/><item id="book" href="book.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="cover"/><itemref idref="book"/></spine></package>"#;
    let ncx = format!(
        r#"<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap>{ncx}</navMap></ncx>"#
    );
    let cover = "<html><body><p>COVER</p></body></html>";
    let book = format!("<html><body>{book}</body></html>");
    support::zip(
        &[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", opf),
            ("OEBPS/toc.ncx", &ncx),
            ("OEBPS/cover.xhtml", cover),
            ("OEBPS/book.xhtml", &book),
        ],
        &["OEBPS/book.xhtml"],
    )
}

/// A book with one document for each chapter and a table of contents without anchors, like
/// "Gulliver's Travels" on the card.
fn per_file_epub(chapters: usize, words_per_chapter: usize) -> Vec<u8> {
    let bodies: Vec<(String, String)> = (1..=chapters)
        .map(|n| {
            (
                format!("Chapter {n}"),
                format!("<h2>Chapter {n}</h2>{}", prose(n, words_per_chapter)),
            )
        })
        .collect();
    let refs: Vec<(&str, &str)> = bodies
        .iter()
        .map(|(t, b)| (t.as_str(), b.as_str()))
        .collect();
    support::simple_epub("One file each", &refs)
}

// ---------------------------------------------------------------------------------------------
// Driving the reader.

/// Opens the book from the list and chooses table of contents entry `toc`, as the buttons would, and
/// returns the page it lands on (document, page, text). Then goes back to the list.
fn land<S: Storage>(lib: &mut BookLibrary<S>, app: &mut App, toc: usize) -> (usize, usize, String) {
    assert_eq!(app.screen(), Screen::List);
    assert_eq!(app.press(Select, lib), Effect::Redraw);
    assert_eq!(app.screen(), Screen::Contents);
    for _ in 0..toc {
        assert_eq!(app.press(Down, lib), Effect::Redraw);
    }
    assert_eq!(app.selected_toc(), toc);
    assert_eq!(app.press(Select, lib), Effect::Redraw);
    assert_eq!(app.screen(), Screen::Reading);
    (
        app.section(),
        app.page(),
        lib.page_text(app).unwrap_or_default(),
    )
}

fn back_to_list<S: Storage>(lib: &mut BookLibrary<S>, app: &mut App) {
    assert_eq!(app.press(Select, lib), Effect::Redraw);
    assert_eq!(app.screen(), Screen::List);
}

#[test]
fn every_chapter_of_a_one_document_book_opens_at_its_own_heading() {
    // Ten chapters of 900 words: each is several pages, so a chapter is nowhere near page 0.
    let chapters = 10;
    let mut mem = Mem::default();
    mem.0
        .insert("book.epub".into(), gutenberg_epub(chapters, 900));
    let mut lib = library(mem, "book.epub");
    let mut app = App::new();

    let mut landed = Vec::new();
    for toc in 0..2 + chapters * 2 {
        let (section, page, text) = land(&mut lib, &mut app, toc);
        let (heading, id) = match toc {
            0 => ("THE BOOK".to_string(), 0),
            1 => ("Contents".to_string(), 1),
            t => {
                let n = (t - 2) / 2 + 1;
                if (t - 2) % 2 == 0 {
                    (format!("CHAPTER {n}"), n)
                } else {
                    (format!("SUBTITLE {n}"), n)
                }
            }
        };
        assert_eq!(section, 1, "entry {toc} ({heading}) is in the big document");
        assert!(
            text.contains(&heading),
            "entry {toc}: the page should hold {heading:?}, it starts: {:.80}",
            text
        );
        if toc >= 2 && page > 0 {
            // It is the first page that holds the heading, not a later one.
            app.press(Up, &mut lib);
            let before = lib.page_text(&app).unwrap_or_default();
            assert!(
                !before.contains(&heading),
                "entry {toc}: {heading:?} is already on the page before"
            );
        }
        landed.push((toc, id, page));
        back_to_list(&mut lib, &mut app);
    }
    // The chapters are in order and far apart: each starts on a later page than the one before.
    let chapter_pages: Vec<usize> = landed
        .iter()
        .filter(|(t, _, _)| *t >= 2 && (t - 2) % 2 == 0)
        .map(|e| e.2)
        .collect();
    assert!(
        chapter_pages.windows(2).all(|w| w[0] < w[1]),
        "chapter pages {chapter_pages:?}"
    );
    assert!(
        chapter_pages[0] <= 1 && *chapter_pages.last().unwrap() > 20,
        "{chapter_pages:?}"
    );
}

#[test]
fn every_chapter_of_a_book_with_a_file_for_each_opens_at_that_file() {
    let mut mem = Mem::default();
    mem.0.insert("book.epub".into(), per_file_epub(8, 300));
    let mut lib = library(mem, "book.epub");
    let mut app = App::new();
    for toc in 0..8 {
        let (section, page, text) = land(&mut lib, &mut app, toc);
        assert_eq!((section, page), (toc, 0), "chapter {}", toc + 1);
        assert!(
            text.starts_with(&format!("Chapter {}", toc + 1)),
            "{:.60}",
            text
        );
        back_to_list(&mut lib, &mut app);
    }
}

#[test]
fn an_anchor_in_the_middle_of_a_paragraph_opens_the_page_with_that_word() {
    // One paragraph of 3000 words with an anchor on a word far into it.
    let words: Vec<String> = (0..3000)
        .map(|k| {
            if k == 2000 {
                r#"<span id="deep">c0w2000</span>"#.to_string()
            } else {
                format!("c0w{k}")
            }
        })
        .collect();
    let doc = format!("<html><body><p>{}</p></body></html>", words.join(" "));
    let opf = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Deep</dc:title></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="d" href="d.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="d"/></spine></package>"#;
    let ncx = r##"<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint id="a"><navLabel><text>Start</text></navLabel><content src="d.xhtml"/></navPoint><navPoint id="b"><navLabel><text>Deep</text></navLabel><content src="d.xhtml#deep"/></navPoint><navPoint id="c"><navLabel><text>Nowhere</text></navLabel><content src="d.xhtml#missing"/></navPoint></navMap></ncx>"##;
    let mut mem = Mem::default();
    mem.0.insert(
        "book.epub".into(),
        support::zip(
            &[
                ("META-INF/container.xml", CONTAINER),
                ("OEBPS/content.opf", opf),
                ("OEBPS/toc.ncx", ncx),
                ("OEBPS/d.xhtml", &doc),
            ],
            &[],
        ),
    );
    let mut lib = library(mem, "book.epub");
    let mut app = App::new();

    let (_, page, text) = land(&mut lib, &mut app, 1);
    assert!(page > 3, "word 2000 is far from the start: page {page}");
    assert!(text.contains("c0w2000"), "the page holds the anchored word");
    app.press(Up, &mut lib);
    assert!(
        !lib.page_text(&app).unwrap().contains("c0w2000"),
        "and it is the first page that does"
    );
    back_to_list(&mut lib, &mut app);

    // An entry without a fragment, and one whose fragment is not in the document, open the start.
    assert_eq!(land(&mut lib, &mut app, 0).1, 0);
    back_to_list(&mut lib, &mut app);
    assert_eq!(
        land(&mut lib, &mut app, 2).1,
        0,
        "an anchor that does not exist"
    );
}

// ---------------------------------------------------------------------------------------------

/// Real books, from `EPUB_TEST_FILES`, read from their files: every table of contents entry that
/// points into the spine is chosen, and the page it opens must hold the start of its title. Titles in
/// a table of contents sometimes differ from the heading, so a few misses are allowed.
#[test]
#[ignore = "needs real books: set EPUB_TEST_FILES"]
fn every_entry_of_real_books_opens_where_its_title_is() {
    let files = std::env::var("EPUB_TEST_FILES").expect("set EPUB_TEST_FILES");
    let normal = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    ' '
                }
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    for path in files.split(':').filter(|p| !p.is_empty()) {
        let mut lib = library(Disk, path);
        let mut app = App::new();
        assert_eq!(app.press(Select, &mut lib), Effect::Redraw, "{path}: opens");
        let entries = lib.toc_entries().to_vec();
        // On to the first page and back to the list, so each entry starts from the list.
        app.press(Select, &mut lib);
        back_to_list(&mut lib, &mut app);
        let toc_len = entries.len();
        let (mut checked, mut matched, mut at_start) = (0, 0, 0);
        let mut misses = Vec::new();
        for (toc, entry) in entries.iter().enumerate() {
            let (section, page, text) = land(&mut lib, &mut app, toc);
            back_to_list(&mut lib, &mut app);
            let (title, in_spine, has_fragment) = (
                entry.title.clone(),
                entry.spine_index.is_some(),
                entry.fragment.is_some(),
            );
            if !in_spine {
                continue;
            }
            checked += 1;
            let head: Vec<String> = normal(&title)
                .split(' ')
                .take(3)
                .map(String::from)
                .collect();
            let head = head.join(" ");
            if head.is_empty() || normal(&text).contains(&head) {
                matched += 1;
            } else if misses.len() < 8 {
                misses.push(format!(
                    "#{toc} {title:?} -> doc {section} page {page}: {:.50}",
                    text
                ));
            }
            if has_fragment && page == 0 && section > 0 {
                at_start += 1;
            }
        }
        println!(
            "{path}: {toc_len} entries, {checked} in the spine, {matched} open on their title, {at_start} with an anchor open at page 0 of their document"
        );
        for m in &misses {
            println!("   miss {m}");
        }
        assert!(checked > 0, "{path}: nothing to check");
        assert!(
            matched * 100 >= checked * 85,
            "{path}: only {matched} of {checked} entries open where their title is"
        );
    }
}
