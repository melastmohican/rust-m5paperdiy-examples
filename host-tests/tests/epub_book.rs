//! Tests of `common/src/epub.rs`: the container, the package and the table of contents, on EPUB 2 and
//! EPUB 3 books built in the test, and on the real books named in `EPUB_TEST_FILES` (see the last test).

use m5paperdiy_common::epub::{Book, EpubError, dir_of, percent_decode, resolve_path};
use m5paperdiy_common::storage::{DirEntry, Storage, StorageError};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------------------------
// A storage in memory, and a zip writer, so a book can be made in the test.

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

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A zip archive of `(name, content)` pairs. Entries named in `deflated` are compressed, the rest
/// are stored, so both paths of the reader are covered.
fn zip(files: &[(&str, &str)], deflated: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, content) in files {
        let raw = content.as_bytes();
        let (method, data) = if deflated.contains(name) {
            (8u16, miniz_oxide::deflate::compress_to_vec(raw, 6))
        } else {
            (0u16, raw.to_vec())
        };
        let crc = crc32(raw);
        let offset = out.len() as u32;
        let mut common = Vec::new();
        common.extend_from_slice(&20u16.to_le_bytes()); // version needed
        common.extend_from_slice(&0u16.to_le_bytes()); // flags
        common.extend_from_slice(&method.to_le_bytes());
        common.extend_from_slice(&0u32.to_le_bytes()); // time and date
        common.extend_from_slice(&crc.to_le_bytes());
        common.extend_from_slice(&(data.len() as u32).to_le_bytes());
        common.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        common.extend_from_slice(&(name.len() as u16).to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes()); // extra length
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&common);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&common);
        central.extend_from_slice(&0u16.to_le_bytes()); // comment length
        central.extend_from_slice(&0u16.to_le_bytes()); // disk number
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let central_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn storage_with(path: &str, archive: Vec<u8>) -> Mem {
    let mut mem = Mem::default();
    mem.0.insert(path.to_string(), archive);
    mem
}

// ---------------------------------------------------------------------------------------------
// Two small books.

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

/// EPUB 2: an NCX with two levels, a file name with a space, a title with an entity.
fn epub2() -> Vec<u8> {
    let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test &amp;
       Book</dc:title><dc:title>Second title</dc:title>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="text/ch%202.xhtml" media-type="application/xhtml+xml"/>
    <item id="c3" href="ch3.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/><itemref idref="c3"/></spine>
</package>"#;
    let ncx = r#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><docTitle><text>Ignored</text></docTitle><navMap>
  <navPoint id="a" playOrder="1"><navLabel><text>Part  One</text></navLabel><content src="ch1.xhtml"/>
    <navPoint id="b" playOrder="2"><navLabel><text>Chapter &amp; Verse</text></navLabel><content src="text/ch%202.xhtml#s1"/></navPoint>
  </navPoint>
  <navPoint id="c" playOrder="3"><navLabel><text>The End</text></navLabel><content src="ch3.xhtml"/></navPoint>
  <navPoint id="d" playOrder="4"><navLabel><text>Elsewhere</text></navLabel><content src="not-in-spine.xhtml"/></navPoint>
</navMap><pageList><pageTarget><navLabel><text>Page 1</text></navLabel><content src="ch1.xhtml"/></pageTarget></pageList></ncx>"#;
    zip(
        &[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", opf),
            ("OEBPS/toc.ncx", ncx),
            ("OEBPS/ch1.xhtml", "<html><body><p>one</p></body></html>"),
            (
                "OEBPS/text/ch 2.xhtml",
                "<html><body><p>two, compressed</p></body></html>",
            ),
            ("OEBPS/ch3.xhtml", "<html><body><p>three</p></body></html>"),
        ],
        &["OEBPS/content.opf", "OEBPS/text/ch 2.xhtml"],
    )
}

/// EPUB 3: no NCX, a `nav` document with a nested list, and a second `nav` that must be ignored.
fn epub3() -> Vec<u8> {
    let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Three</dc:title></metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="a" href="a.xhtml" media-type="application/xhtml+xml"/>
    <item id="b" href="b.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="a"/><itemref idref="b"/></spine>
</package>"#;
    let nav = r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="landmarks"><ol><li><a href="a.xhtml">Start here</a></li></ol></nav>
<nav epub:type="toc" id="toc"><ol>
  <li><a href="a.xhtml">One</a>
    <ol><li><a href="b.xhtml#sec">Two <em>and</em>
      a half</a></li></ol></li>
</ol></nav></body></html>"#;
    zip(
        &[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", opf),
            ("OEBPS/nav.xhtml", nav),
            ("OEBPS/a.xhtml", "<p>a</p>"),
            ("OEBPS/b.xhtml", "<p>b</p>"),
        ],
        &[],
    )
}

// ---------------------------------------------------------------------------------------------

#[test]
fn epub2_has_the_title_the_spine_and_the_table_of_contents() {
    let mem = storage_with("book.epub", epub2());
    let book = Book::open(&mem, "book.epub").unwrap();
    // Entities are decoded, the line break collapses, and only the first title counts.
    assert_eq!(book.title(), "Test & Book");
    assert_eq!(
        book.spine(),
        [
            "OEBPS/ch1.xhtml",
            "OEBPS/text/ch 2.xhtml",
            "OEBPS/ch3.xhtml"
        ]
    );
    let toc: Vec<_> = book
        .toc()
        .iter()
        .map(|e| {
            (
                e.title.as_str(),
                e.level,
                e.spine_index,
                e.fragment.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        toc,
        [
            ("Part One", 0, Some(0), None),
            ("Chapter & Verse", 1, Some(1), Some("s1")),
            ("The End", 0, Some(2), None),
            // Not in the spine, but still listed. The page list is not part of the contents.
            ("Elsewhere", 0, None, None),
        ]
    );
}

#[test]
fn epub2_documents_are_read_stored_and_deflated() {
    let mem = storage_with("book.epub", epub2());
    let book = Book::open(&mem, "book.epub").unwrap();
    assert_eq!(
        book.read_spine(&mem, 0).unwrap(),
        b"<html><body><p>one</p></body></html>"
    );
    assert_eq!(
        book.read_spine(&mem, 1).unwrap(),
        b"<html><body><p>two, compressed</p></body></html>"
    );
    assert_eq!(book.read_spine(&mem, 3), Err(EpubError::OutOfRange));
}

#[test]
fn epub3_uses_the_nav_document_and_its_toc_list() {
    let mem = storage_with("book.epub", epub3());
    let book = Book::open(&mem, "book.epub").unwrap();
    assert_eq!(book.title(), "Three");
    let toc: Vec<_> = book
        .toc()
        .iter()
        .map(|e| {
            (
                e.title.as_str(),
                e.level,
                e.spine_index,
                e.fragment.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        toc,
        [
            ("One", 0, Some(0), None),
            // Text inside an inline element joins the title and white space collapses.
            ("Two and a half", 1, Some(1), Some("sec")),
        ]
    );
}

#[test]
fn the_first_nav_is_used_when_none_says_toc() {
    let opf = r#"<package><metadata><dc:title>T</dc:title></metadata><manifest>
        <item id="n" href="n.xhtml" properties="nav"/><item id="a" href="a.xhtml"/></manifest>
        <spine><itemref idref="a"/></spine></package>"#;
    let nav =
        r#"<html><body><nav><ol><li><a href="a.xhtml">Only</a></li></ol></nav></body></html>"#;
    let archive = zip(
        &[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", opf),
            ("OEBPS/n.xhtml", nav),
            ("OEBPS/a.xhtml", "x"),
        ],
        &[],
    );
    let mem = storage_with("b.epub", archive);
    let book = Book::open(&mem, "b.epub").unwrap();
    assert_eq!(book.toc().len(), 1);
    assert_eq!(book.toc()[0].title, "Only");
}

#[test]
fn a_book_without_a_table_of_contents_has_an_empty_one() {
    let opf = r#"<package><metadata><dc:title>T</dc:title></metadata><manifest>
        <item id="a" href="a.xhtml"/></manifest><spine><itemref idref="a"/></spine></package>"#;
    let archive = zip(
        &[
            ("META-INF/container.xml", CONTAINER),
            ("OEBPS/content.opf", opf),
            ("OEBPS/a.xhtml", "x"),
        ],
        &[],
    );
    let mem = storage_with("b.epub", archive);
    let book = Book::open(&mem, "b.epub").unwrap();
    assert_eq!(book.spine(), ["OEBPS/a.xhtml"]);
    assert!(book.toc().is_empty());
}

#[test]
fn bad_books_are_refused_with_a_reason() {
    // Not a zip archive at all.
    let mem = storage_with("x.epub", b"this is not a zip file, just some text".to_vec());
    assert!(matches!(Book::open(&mem, "x.epub"), Err(EpubError::Zip(_))));

    // A zip without container.xml.
    let mem = storage_with("x.epub", zip(&[("mimetype", "application/epub+zip")], &[]));
    assert_eq!(
        Book::open(&mem, "x.epub").err(),
        Some(EpubError::Missing("META-INF/container.xml"))
    );

    // A container that names no package.
    let mem = storage_with(
        "x.epub",
        zip(
            &[(
                "META-INF/container.xml",
                "<container><rootfiles/></container>",
            )],
            &[],
        ),
    );
    assert!(matches!(
        Book::open(&mem, "x.epub"),
        Err(EpubError::Invalid(_))
    ));

    // A package that is not in the archive.
    let mem = storage_with("x.epub", zip(&[("META-INF/container.xml", CONTAINER)], &[]));
    assert_eq!(
        Book::open(&mem, "x.epub").err(),
        Some(EpubError::Missing("package document"))
    );

    // A file that does not exist in the storage.
    assert!(Book::open(&Mem::default(), "nothing.epub").is_err());
}

#[test]
fn damaged_archives_never_panic() {
    let good = epub2();
    // Cut the archive short at every length, and flip one byte at a time.
    for cut in 0..good.len() {
        let mem = storage_with("b.epub", good[..cut].to_vec());
        let _ = Book::open(&mem, "b.epub");
    }
    for at in (0..good.len()).step_by(3) {
        let mut bad = good.clone();
        bad[at] ^= 0xA5;
        let mem = storage_with("b.epub", bad);
        if let Ok(book) = Book::open(&mem, "b.epub") {
            for i in 0..book.spine().len() {
                let _ = book.read_spine(&mem, i);
            }
        }
    }
}

#[test]
fn paths_follow_dots_and_percent_escapes() {
    assert_eq!(dir_of("OEBPS/text/ch1.xhtml"), "OEBPS/text/");
    assert_eq!(dir_of("content.opf"), "");
    assert_eq!(resolve_path("OEBPS/", "ch1.xhtml"), "OEBPS/ch1.xhtml");
    assert_eq!(
        resolve_path("OEBPS/text/", "../img/a.png"),
        "OEBPS/img/a.png"
    );
    assert_eq!(resolve_path("OEBPS/", "./a/./b.xhtml"), "OEBPS/a/b.xhtml");
    assert_eq!(resolve_path("OEBPS/", "/abs/x.xhtml"), "abs/x.xhtml");
    assert_eq!(resolve_path("", "a%20b.xhtml"), "a b.xhtml");
    // Too many `..` stay at the top instead of panicking.
    assert_eq!(resolve_path("a/", "../../../x"), "x");
    assert_eq!(percent_decode("%C3%A9t%C3%A9"), "été");
    assert_eq!(percent_decode("100%"), "100%");
    assert_eq!(percent_decode("%zz%4"), "%zz%4");
}

/// Real books, read from the paths in `EPUB_TEST_FILES` (separated by `:`). They are not in the
/// repository. Run with:
///
/// ```text
/// EPUB_TEST_FILES="$HOME/Documents/eBook/Microsoft/0735627398.epub:..." host-tests/run.sh -- --ignored
/// ```
#[test]
#[ignore = "needs real books: set EPUB_TEST_FILES"]
fn real_books_have_a_title_and_a_table_of_contents() {
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
    for path in files.split(':').filter(|p| !p.is_empty()) {
        let book = Book::open(&Disk, path).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(!book.title().is_empty(), "{path}: empty title");
        assert!(!book.toc().is_empty(), "{path}: empty table of contents");
        assert!(!book.spine().is_empty(), "{path}: empty spine");
        // Every spine document inflates, and a table of contents entry points into the spine.
        for i in 0..book.spine().len() {
            book.read_spine(&Disk, i)
                .unwrap_or_else(|e| panic!("{path}: spine {i}: {e}"));
        }
        assert!(
            book.toc().iter().any(|e| e.spine_index.is_some()),
            "{path}: no table of contents entry resolves to the spine"
        );
        println!(
            "{path}: {:?}, {} spine items, {} toc entries",
            book.title(),
            book.spine().len(),
            book.toc().len()
        );
    }
}
