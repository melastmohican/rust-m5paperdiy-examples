//! Tests of `common/src/html.rs` and `common/src/layout.rs`: a chapter of XHTML to blocks, and the
//! blocks to lines and pages, with fake font widths so no board is needed.

use m5paperdiy_common::html::{Align, Block, TextBlock, parse};
use m5paperdiy_common::layout::{Draw, Layout, TextMetrics};

/// A font where a character is 8 pixels wide (9 in bold), a space 4, and a line 20 high.
struct Fake;

impl TextMetrics for Fake {
    fn text_width(&self, text: &str, bold: bool, _italic: bool) -> u32 {
        text.chars().count() as u32 * if bold { 9 } else { 8 }
    }
    fn space_width(&self) -> u32 {
        4
    }
    fn line_height(&self) -> u32 {
        20
    }
}

fn no_images(_: &str) -> Option<(u32, u32)> {
    None
}

fn texts(blocks: &[Block]) -> Vec<&TextBlock> {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text(t) => Some(t),
            Block::Image(_) => None,
        })
        .collect()
}

fn words(block: &TextBlock) -> Vec<String> {
    block.words().map(|(w, _, _)| w.to_string()).collect()
}

fn lay(html: &str, width: u32, height: u32) -> Layout {
    Layout::new(parse(html.as_bytes(), ""), &Fake, width, height, &no_images)
}

/// What a page shows: for each line, its y and its words with their x.
fn page(layout: &Layout, index: usize) -> Vec<(i32, Vec<(i32, String)>)> {
    let mut lines: Vec<(i32, Vec<(i32, String)>)> = Vec::new();
    layout.draw_page(index, &mut |d| {
        if let Draw::Word { x, y, text, .. } = d {
            match lines.last_mut() {
                Some((ly, words)) if *ly == y => words.push((x, text.to_string())),
                _ => lines.push((y, vec![(x, text.to_string())])),
            }
        }
    });
    lines
}

fn xorshift(mut state: u32) -> impl FnMut() -> u32 {
    move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    }
}

// ---------------------------------------------------------------------------------------------
// Parsing

#[test]
fn paragraphs_headings_and_breaks_make_blocks() {
    let blocks = parse(
        b"<html><head><title>T</title></head><body><h1>Title <i>x</i></h1>\
          <p>Hello  big\n world</p><p></p><p>Second<br/>third</p></body></html>",
        "",
    );
    let blocks = texts(&blocks);
    assert_eq!(blocks.len(), 4, "the empty paragraph makes no block");
    assert_eq!(blocks[0].align, Align::Center);
    assert_eq!(
        blocks[0].words().collect::<Vec<_>>(),
        [("Title", true, false), ("x", true, true)],
        "a heading is bold"
    );
    assert_eq!(words(blocks[1]), ["Hello", "big", "world"]);
    assert_eq!(blocks[1].align, Align::Justified);
    // A `br` starts a block with the same alignment.
    assert_eq!(words(blocks[2]), ["Second"]);
    assert_eq!(words(blocks[3]), ["third"]);
    assert_eq!(blocks[3].align, Align::Justified);
}

#[test]
fn bold_and_italic_nest() {
    let blocks = parse(
        b"<p>a <b>b <i>bi</i> b2</b> c <strong>s</strong> <em>e</em></p>",
        "",
    );
    let words: Vec<_> = texts(&blocks)[0].words().collect();
    assert_eq!(
        words,
        [
            ("a", false, false),
            ("b", true, false),
            ("bi", true, true),
            // `</i>` does not switch bold off.
            ("b2", true, false),
            ("c", false, false),
            ("s", true, false),
            ("e", false, true),
        ]
    );
}

#[test]
fn entities_are_decoded_and_no_break_spaces_stay_in_their_word() {
    let blocks = parse(
        "<p>Tom &amp; Jerry&#8212;ok &nbsp;x a&nbsp;b</p>".as_bytes(),
        "",
    );
    assert_eq!(
        words(texts(&blocks)[0]),
        ["Tom", "&", "Jerry\u{2014}ok", "\u{a0}x", "a\u{a0}b"]
    );
}

#[test]
fn head_script_style_and_table_are_skipped_with_what_is_in_them() {
    let blocks = parse(
        b"<html><head><title>no</title></head><body><p>yes</p>\
          <table><tr><td>cell <b>bold</b></td></tr></table>\
          <script>var a = 1 < 2;</script><style>p { color: red }</style><p>after</p></body></html>",
        "",
    );
    let all: Vec<String> = texts(&blocks).iter().flat_map(|b| words(b)).collect();
    assert_eq!(all, ["yes", "after"]);
}

#[test]
fn an_image_splits_the_text_and_keeps_the_alignment() {
    let blocks = parse(
        b"<div>before<img src=\"../img/a%20b.png\"/>after</div><h2>t</h2><img/><img src=\"x.png\">z",
        "OEBPS/text/",
    );
    let shape: Vec<String> = blocks
        .iter()
        .map(|b| match b {
            Block::Text(t) => format!("text {:?} {:?}", t.align, words(t)),
            Block::Image(i) => format!("image {}", i.path),
        })
        .collect();
    assert_eq!(
        shape,
        [
            "text Justified [\"before\"]",
            "image OEBPS/img/a b.png",
            "text Justified [\"after\"]",
            "text Center [\"t\"]",
            // The `img` without `src` is ignored. The text after the last image keeps the
            // alignment of the heading.
            "image OEBPS/text/x.png",
            "text Center [\"z\"]",
        ]
    );
}

#[test]
fn a_long_chapter_keeps_every_word_whole() {
    // The tokenizer hands text over in pieces. A word must never be cut at a piece.
    let mut html = String::from("<html><body><p>");
    let mut expected = Vec::new();
    for i in 0..30_000 {
        let word = format!("word{i}");
        html.push_str(&word);
        html.push(if i % 7 == 0 { '\n' } else { ' ' });
        expected.push(word);
    }
    html.push_str("</p></body></html>");
    let blocks = parse(html.as_bytes(), "");
    let blocks = texts(&blocks);
    assert_eq!(blocks.len(), 1);
    assert_eq!(words(blocks[0]), expected);
}

// ---------------------------------------------------------------------------------------------
// Lines

/// The cost of a line of words `from..to` as the layout counts it, `None` when it is too wide.
fn line_cost(
    widths: &[u32],
    from: usize,
    to: usize,
    space: u32,
    page: u32,
    is_last: bool,
) -> Option<u64> {
    let mut current = -1i64;
    for w in &widths[from..to] {
        current += i64::from(*w) + i64::from(space);
    }
    if current > i64::from(page) && to - from > 1 {
        return None;
    }
    Some(if is_last {
        0
    } else {
        let left = (i64::from(page) - current).max(0) as u64;
        left * left
    })
}

/// The lowest cost of laying out `widths[from..]`, by trying every way.
fn best_cost(widths: &[u32], from: usize, space: u32, page: u32) -> u64 {
    if from == widths.len() {
        return 0;
    }
    let mut best = u64::MAX;
    for to in from + 1..=widths.len() {
        if let Some(cost) = line_cost(widths, from, to, space, page, to == widths.len()) {
            let rest = best_cost(widths, to, space, page);
            best = best.min(cost.saturating_add(rest));
        }
    }
    best
}

#[test]
fn the_line_breaks_cost_as_little_as_any_other_breaks() {
    let mut random = xorshift(0x2545_F491);
    for round in 0..300 {
        let page = 60 + (random() % 140);
        let count = 1 + (random() % 9) as usize;
        let text: Vec<String> = (0..count)
            .map(|_| "x".repeat(1 + (random() % 9) as usize))
            .collect();
        let html = format!("<p>{}</p>", text.join(" "));
        let blocks = parse(html.as_bytes(), "");
        let layout = Layout::new(blocks, &Fake, page, 100_000, &no_images);

        // The lines the layout made, as word counts.
        let mut lines: Vec<usize> = Vec::new();
        layout.draw_page(0, &mut |d| {
            if let Draw::Word { y, .. } = d {
                if lines.len() <= (y / 20) as usize {
                    lines.push(0);
                }
                let last = lines.len() - 1;
                lines[last] += 1;
            }
        });
        let widths: Vec<u32> = text.iter().map(|w| w.len() as u32 * 8).collect();
        let mut from = 0;
        let mut cost = 0u64;
        for (n, count) in lines.iter().enumerate() {
            cost += line_cost(&widths, from, from + count, 4, page, n + 1 == lines.len())
                .unwrap_or_else(|| panic!("round {round}: a line is too wide"));
            from += count;
        }
        assert_eq!(from, count, "round {round}: every word is on a line");
        assert_eq!(
            cost,
            best_cost(&widths, 0, 4, page),
            "round {round}: {text:?} at {page}"
        );
    }
}

#[test]
fn words_keep_their_order_and_lines_reach_the_edges_when_justified() {
    let mut random = xorshift(0x9E37_79B9);
    for round in 0..200 {
        let page = 120 + (random() % 200);
        let text: Vec<String> = (0..(5 + random() % 60))
            .map(|_| "y".repeat(1 + (random() % 10) as usize))
            .collect();
        let html = format!("<p>{}</p>", text.join(" "));
        let layout = lay(&html, page, 100_000);
        let lines = page_lines(&layout, 0);
        let flat: Vec<&String> = lines
            .iter()
            .flat_map(|l| l.1.iter().map(|(_, w)| w))
            .collect();
        assert_eq!(flat.len(), text.len(), "round {round}");
        for (a, b) in flat.iter().zip(&text) {
            assert_eq!(*a, b, "round {round}: order");
        }
        for (n, (_, line)) in lines.iter().enumerate() {
            let (first_x, _) = line[0];
            let (last_x, last_word) = line.last().unwrap();
            let end = last_x + (last_word.len() as i32) * 8;
            assert_eq!(
                first_x, 0,
                "round {round}: a justified line starts at the edge"
            );
            assert!(
                end <= page as i32,
                "round {round}: line {n} fits, ends at {end} of {page}"
            );
            if n + 1 < lines.len() && line.len() > 1 {
                // Stretched to the full width, give or take the pixel lost by rounding down.
                assert!(
                    end >= page as i32 - 1,
                    "round {round}: line {n} ends at {end} of {page}"
                );
            }
        }
    }
}

fn page_lines(layout: &Layout, index: usize) -> Vec<(i32, Vec<(i32, String)>)> {
    page(layout, index)
}

#[test]
fn alignment_places_a_line_as_the_arduino_reader_does() {
    // "aa" and "bb" are 16 wide each, the space is 4, the page 100: 68 is left over.
    let place = |tag: &str| {
        let layout = lay(&format!("<{tag}>aa bb</{tag}>"), 100, 1000);
        page(&layout, 0)[0]
            .1
            .iter()
            .map(|(x, _)| *x)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        place("p"),
        [0, 20],
        "the last line of a justified paragraph is not stretched"
    );
    // A heading is bold, so a word is 18 wide: 64 is left over, and (64 - 4) / 2 = 30.
    assert_eq!(place("h1"), [30, 52], "centered");
    // Left and right alignment only come from headings and from the block's own style, which the
    // parser sets to justified or centered, so check right alignment on the block directly.
    let mut blocks = parse(b"<p>aa bb</p>", "");
    if let Block::Text(t) = &mut blocks[0] {
        t.align = Align::Right;
    }
    let layout = Layout::new(blocks, &Fake, 100, 1000, &no_images);
    let xs: Vec<i32> = page(&layout, 0)[0].1.iter().map(|(x, _)| *x).collect();
    assert_eq!(xs, [64, 84], "right: 68 - 4");
    let mut blocks = parse(b"<p>aa bb</p>", "");
    if let Block::Text(t) = &mut blocks[0] {
        t.align = Align::Left;
    }
    let layout = Layout::new(blocks, &Fake, 100, 1000, &no_images);
    let xs: Vec<i32> = page(&layout, 0)[0].1.iter().map(|(x, _)| *x).collect();
    assert_eq!(xs, [0, 20]);
}

#[test]
fn a_word_wider_than_the_line_has_a_line_of_its_own() {
    // "abcdefghij" is 80 wide on a page of 40.
    let layout = lay("<p>abcdefghij x y</p>", 40, 1000);
    let lines = page(&layout, 0);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].1, [(0, "abcdefghij".to_string())]);
    assert_eq!(lines[1].1.len(), 2);
}

// ---------------------------------------------------------------------------------------------
// Pages

/// A page 30 wide holds one "xx" (16) per line, so a paragraph of N words is N lines.
fn paragraph(words: usize) -> String {
    format!("<p>{}</p>", vec!["xx"; words].join(" "))
}

#[test]
fn paragraphs_have_half_a_line_between_them_and_lines_flow_to_the_next_page() {
    // Lines are 20 high, a page 100 high, the gap is 10.
    let html = format!("{}{}{}", paragraph(2), paragraph(3), paragraph(1));
    let layout = lay(&html, 30, 100);
    let ys = |i| page(&layout, i).iter().map(|l| l.0).collect::<Vec<_>>();
    // Paragraph 1: 0, 20, gap to 50. Paragraph 2: 50, 70, then 90 does not fit (90 + 20 > 100).
    assert_eq!(ys(0), [0, 20, 50, 70]);
    // Its third line starts the next page, then the gap, then paragraph 3.
    assert_eq!(ys(1), [0, 30]);
    assert_eq!(layout.page_count(), 2);
}

#[test]
fn there_is_never_an_empty_page() {
    // A page shorter than a line: every line is alone on a page, and none is empty.
    let layout = lay(&paragraph(3), 30, 10);
    assert_eq!(layout.page_count(), 3);
    for i in 0..3 {
        assert_eq!(page(&layout, i).len(), 1);
    }
    // A chapter without anything to show has no pages, and drawing past the end does nothing.
    let empty = lay("<html><body><p> </p></body></html>", 30, 100);
    assert_eq!(empty.page_count(), 0);
    let mut drawn = 0;
    empty.draw_page(0, &mut |_| drawn += 1);
    layout.draw_page(99, &mut |_| drawn += 1);
    assert_eq!(drawn, 0);
}

#[test]
fn images_are_scaled_centered_paginated_and_missing_ones_left_out() {
    let sizes = |path: &str| match path {
        "big.png" => Some((400, 200)),
        "small.png" => Some((50, 50)),
        _ => None,
    };
    let html = r#"<p>t</p><img src="big.png"/><img src="small.png"/><img src="missing.png"/><img src="small.png"/>"#;
    let layout = Layout::new(parse(html.as_bytes(), ""), &Fake, 100, 100, &sizes);
    let mut images = Vec::new();
    for p in 0..layout.page_count() {
        layout.draw_page(p, &mut |d| {
            if let Draw::Image {
                path,
                x,
                y,
                width,
                height,
            } = d
            {
                images.push((p, path.to_string(), x, y, width, height));
            }
        });
    }
    assert_eq!(
        images,
        [
            // 400 x 200 scales by 0.25 to 100 x 50. The text line and gap take 30, so 30 + 50 > 100
            // is false and it stays on page 0.
            (0, "big.png".to_string(), 0, 30, 100, 50),
            // 50 x 50 does not fit under it (80 + 50 > 100): page 1, centered.
            (1, "small.png".to_string(), 25, 0, 50, 50),
            // The missing one is skipped, the next small one goes under the first (50 + 50 <= 100).
            (1, "small.png".to_string(), 25, 50, 50, 50),
        ]
    );
}

#[test]
fn draw_page_reports_words_with_their_styles_in_reading_order() {
    let layout = lay("<p>one <b>two</b> <i>three</i></p>", 400, 100);
    let mut drawn = Vec::new();
    layout.draw_page(0, &mut |d| {
        if let Draw::Word {
            text,
            bold,
            italic,
            y,
            ..
        } = d
        {
            drawn.push((text.to_string(), bold, italic, y));
        }
    });
    assert_eq!(
        drawn,
        [
            ("one".to_string(), false, false, 0),
            ("two".to_string(), true, false, 0),
            ("three".to_string(), false, true, 0),
        ]
    );
}

// ---------------------------------------------------------------------------------------------

/// Real books, read from the paths in `EPUB_TEST_FILES` (separated by `:`): every chapter of each
/// is parsed and laid out on a page of 480 by 800 pixels with the fake font.
#[test]
#[ignore = "needs real books: set EPUB_TEST_FILES"]
fn every_chapter_of_real_books_lays_out_within_the_page() {
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
    for path in files.split(':').filter(|p| !p.is_empty()) {
        let book = Book::open(&Disk, path).unwrap();
        let (mut chapters, mut pages, mut words, mut wide) = (0, 0, 0, 0);
        let started = std::time::Instant::now();
        for i in 0..book.spine().len() {
            let data = book.read_spine(&Disk, i).unwrap();
            let blocks = parse(&data, dir_of(&book.spine()[i]));
            words += texts(&blocks).iter().map(|b| b.len()).sum::<usize>();
            let layout = Layout::new(blocks, &Fake, 480, 800, &|_| Some((300, 200)));
            chapters += 1;
            pages += layout.page_count();
            for p in 0..layout.page_count() {
                // The words of the page by line, with the style, to measure each one as it is drawn.
                let mut lines: Vec<(i32, Vec<(i32, u32)>)> = Vec::new();
                layout.draw_page(p, &mut |d| {
                    if let Draw::Word {
                        x,
                        y,
                        text,
                        bold,
                        italic,
                    } = d
                    {
                        let end = x + Fake.text_width(text, bold, italic) as i32;
                        match lines.last_mut() {
                            Some((ly, words)) if *ly == y => words.push((x, end as u32)),
                            _ => lines.push((y, vec![(x, end as u32)])),
                        }
                    }
                });
                for (y, line) in lines {
                    if line.len() > 1 {
                        let end = line.last().map_or(0, |w| w.1);
                        assert!(
                            end <= 480,
                            "{path}: page {p}, y {y}: a line of {} words ends at {end}",
                            line.len()
                        );
                    } else {
                        wide += 1;
                    }
                }
            }
        }
        println!(
            "{path}: {chapters} chapters, {pages} pages, {words} words, {wide} single-word lines, {:?}",
            started.elapsed()
        );
        assert!(pages > 0 && words > 0, "{path}: nothing to read");
    }
}
