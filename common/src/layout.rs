//! Blocks to pages: where each line of a paragraph breaks, where each word sits on its line, and
//! which lines go on which page. This is `TextBlock::layout` and `RubbishHtmlParser::layout` of the
//! Arduino reader, with the font reduced to a trait so the host tests can use fake widths.
//!
//! Lines break by dynamic programming: the cost of a line is the square of the space left at the
//! end of it, the last line of a paragraph costs nothing, and the breaks with the lowest total cost
//! win. That gives even lines instead of the long and short ones a greedy fill makes.

use alloc::vec;
use alloc::vec::Vec;

use crate::html::{Align, Anchor, Block, TextBlock};

/// What the layout needs to know about the font, in pixels.
pub trait TextMetrics {
    /// Width of `text` in the given style.
    fn text_width(&self, text: &str, bold: bool, italic: bool) -> u32;
    /// Width of the space between two words.
    fn space_width(&self) -> u32;
    /// Height of a line, with the space under it.
    fn line_height(&self) -> u32;
}

/// Something to put on a page, found by block and line so the words are not copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageItem {
    Line { block: u32, line: u32, y: u32 },
    Image { block: u32, y: u32 },
}

#[derive(Debug, Default)]
struct Page {
    items: Vec<PageItem>,
}

/// One thing to draw, as [`Layout::draw_page`] reports it. Coordinates are from the top left of the
/// page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Draw<'a> {
    Word {
        x: i32,
        y: i32,
        text: &'a str,
        bold: bool,
        italic: bool,
    },
    Image {
        path: &'a str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
}

/// A chapter laid out in pages.
#[derive(Debug)]
pub struct Layout {
    blocks: Vec<Block>,
    pages: Vec<Page>,
    anchors: Vec<Anchor>,
}

impl Layout {
    /// Lays out `blocks` on pages of `page_width` by `page_height` pixels.
    ///
    /// `image_size` gives the size of an image by its path inside the archive, `None` when it
    /// cannot be read; such an image is left out. An image larger than the page is scaled down to
    /// fit and centered.
    pub fn new(
        blocks: Vec<Block>,
        metrics: &dyn TextMetrics,
        page_width: u32,
        page_height: u32,
        image_size: &dyn Fn(&str) -> Option<(u32, u32)>,
    ) -> Self {
        Self::with_anchors(
            blocks,
            Vec::new(),
            metrics,
            page_width,
            page_height,
            image_size,
        )
    }

    /// As [`Layout::new`], and remembers the `anchors` of the chapter, so [`Layout::page_of_anchor`]
    /// can say which page each one is on.
    pub fn with_anchors(
        mut blocks: Vec<Block>,
        anchors: Vec<Anchor>,
        metrics: &dyn TextMetrics,
        page_width: u32,
        page_height: u32,
        image_size: &dyn Fn(&str) -> Option<(u32, u32)>,
    ) -> Self {
        for block in &mut blocks {
            match block {
                Block::Text(text) => text.layout(metrics, page_width),
                Block::Image(image) => {
                    let Some((mut width, mut height)) = image_size(&image.path) else {
                        continue;
                    };
                    if width > page_width || height > page_height {
                        let scale = (page_width as f32 / width as f32)
                            .min(page_height as f32 / height as f32);
                        width = (width as f32 * scale) as u32;
                        height = (height as f32 * scale) as u32;
                    }
                    image.width = width;
                    image.height = height;
                    image.x = page_width.saturating_sub(width) / 2;
                }
            }
        }

        let line_height = metrics.line_height();
        let mut pages: Vec<Page> = Vec::new();
        let mut page = Page::default();
        let mut y = 0u32;
        // Starts a new page, unless this one is still empty.
        let mut next_page = |page: &mut Page, y: &mut u32| {
            if !page.items.is_empty() {
                pages.push(core::mem::take(page));
            }
            *y = 0;
        };
        for (index, block) in blocks.iter().enumerate() {
            match block {
                Block::Text(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    for line in 0..text.line_breaks.len() {
                        if y + line_height > page_height {
                            next_page(&mut page, &mut y);
                        }
                        page.items.push(PageItem::Line {
                            block: index as u32,
                            line: line as u32,
                            y,
                        });
                        y += line_height;
                    }
                    // Half a line between paragraphs.
                    y += line_height / 2;
                }
                Block::Image(image) => {
                    if image.height == 0 {
                        continue;
                    }
                    if y + image.height > page_height {
                        next_page(&mut page, &mut y);
                    }
                    page.items.push(PageItem::Image {
                        block: index as u32,
                        y,
                    });
                    y += image.height;
                }
            }
        }
        if !page.items.is_empty() {
            pages.push(page);
        }
        Self {
            blocks,
            pages,
            anchors,
        }
    }

    /// The page that holds the anchor `id`: the first page that has the line the anchor is on, or
    /// the one after it when that line is not on any page. `None` when the chapter has no such
    /// anchor.
    pub fn page_of_anchor(&self, id: &str) -> Option<usize> {
        let anchor = self.anchors.iter().find(|a| a.id == id)?;
        let (mut block, mut line) = (anchor.block as usize, 0);
        // The line of the block that holds the word, 0 for an image. A position after the last word
        // of a block is the start of the next one.
        if let Some(Block::Text(text)) = self.blocks.get(block) {
            match text.line_breaks.iter().position(|&end| anchor.word < end) {
                Some(found) => line = found,
                None => block += 1,
            }
        }
        for (index, page) in self.pages.iter().enumerate() {
            for item in &page.items {
                let at = match *item {
                    PageItem::Line { block, line, .. } => (block as usize, line as usize),
                    PageItem::Image { block, .. } => (block as usize, 0),
                };
                if at >= (block, line) {
                    return Some(index);
                }
            }
        }
        // Past everything on the pages: the last page.
        self.pages.len().checked_sub(1)
    }

    /// Number of pages, 0 for a chapter without text or images.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Reports everything on page `index` to `draw`, from the top down. Does nothing past the last
    /// page.
    pub fn draw_page(&self, index: usize, draw: &mut dyn FnMut(Draw<'_>)) {
        let Some(page) = self.pages.get(index) else {
            return;
        };
        for item in &page.items {
            match *item {
                PageItem::Line { block, line, y } => {
                    let Some(Block::Text(text)) = self.blocks.get(block as usize) else {
                        continue;
                    };
                    let line = line as usize;
                    let start = if line == 0 {
                        0
                    } else {
                        text.line_breaks[line - 1] as usize
                    };
                    let end = text.line_breaks[line] as usize;
                    for i in start..end {
                        draw(Draw::Word {
                            x: i32::from(text.word_x[i]),
                            y: y as i32,
                            text: text.word(i),
                            bold: text.words[i].bold,
                            italic: text.words[i].italic,
                        });
                    }
                }
                PageItem::Image { block, y } => {
                    let Some(Block::Image(image)) = self.blocks.get(block as usize) else {
                        continue;
                    };
                    draw(Draw::Image {
                        path: &image.path,
                        x: image.x as i32,
                        y: y as i32,
                        width: image.width,
                        height: image.height,
                    });
                }
            }
        }
    }
}

/// Breaks `text` into lines of at most `width` pixels, left aligned, with the same line breaker as
/// a paragraph. A word wider than `width` has a line of its own. For titles in lists.
pub fn wrap_text(text: &str, metrics: &dyn TextMetrics, width: u32) -> Vec<alloc::string::String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let widths: Vec<u32> = words
        .iter()
        .map(|word| metrics.text_width(word, false, false))
        .collect();
    let mut lines = Vec::new();
    let mut start = 0usize;
    for end in break_lines(&widths, metrics.space_width(), width) {
        lines.push(words[start..end as usize].join(" "));
        start = end as usize;
    }
    lines
}

impl TextBlock {
    /// Works out the line breaks and the position of every word for a line `page_width` wide.
    pub(crate) fn layout(&mut self, metrics: &dyn TextMetrics, page_width: u32) {
        let widths: Vec<u32> = self
            .words()
            .map(|(word, bold, italic)| metrics.text_width(word, bold, italic))
            .collect();
        let space = metrics.space_width();
        self.line_breaks = break_lines(&widths, space, page_width);

        // Place the words along each line.
        self.word_x = vec![0; widths.len()];
        let mut start = 0usize;
        for (n, &end) in self.line_breaks.iter().enumerate() {
            let end = end as usize;
            let count = end - start;
            let total: u32 = widths[start..end].iter().sum();
            let spare = page_width as f32 - total as f32;
            let mut spacing = space as f32;
            let is_last = n + 1 == self.line_breaks.len();
            // The last line of a justified paragraph is not stretched.
            if !is_last && self.align == Align::Justified && count > 1 {
                spacing = spare / (count - 1) as f32;
            }
            let gaps = (count - 1) as f32 * space as f32;
            let mut x = match self.align {
                Align::Right => spare - gaps,
                Align::Center => (spare - gaps) / 2.0,
                Align::Justified | Align::Left => 0.0,
            };
            for (word, &width) in widths.iter().enumerate().take(end).skip(start) {
                // A float to `u16` saturates, so a word wider than the line sits at 0.
                self.word_x[word] = x as u16;
                x += width as f32 + spacing;
            }
            start = end;
        }
    }
}

/// The end of each line, as an index one past its last word. A line holds at least one word, even
/// one wider than `page_width`.
///
/// The width of a line of words `i` to `j` is counted as the Arduino reader counts it: each word
/// and a space after it, minus 1.
fn break_lines(widths: &[u32], space: u32, page_width: u32) -> Vec<u32> {
    let n = widths.len();
    if n == 0 {
        return Vec::new();
    }
    let page = i64::from(page_width);
    // `cost[i]`: the lowest cost of laying out words `i..`. `last[i]`: the last word of the first
    // line of that layout.
    let mut cost = vec![0u64; n];
    let mut last: Vec<usize> = (0..n).collect();
    for i in (0..n - 1).rev() {
        cost[i] = u64::MAX;
        let mut current = -1i64;
        for j in i..n {
            current += i64::from(widths[j]) + i64::from(space);
            let too_wide = current > page;
            if too_wide && j > i {
                break;
            }
            let line_cost = if j == n - 1 {
                0
            } else {
                let left = (page - current).max(0) as u64;
                left * left + cost[j + 1]
            };
            if line_cost < cost[i] {
                cost[i] = line_cost;
                last[i] = j;
            }
            if too_wide {
                // A single word wider than the line, there is no longer line to try.
                break;
            }
        }
    }
    let mut breaks = Vec::new();
    let mut i = 0;
    while i < n {
        i = last[i] + 1;
        breaks.push(i as u32);
    }
    breaks
}
