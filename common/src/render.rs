//! Drawing the reader's screens: a page of a book, the list of books, the table of contents and two
//! small notices. The target is any `embedded-graphics` draw target in 4 bit gray, so the host tests
//! can draw into memory and look at the pictures.
//!
//! The geometry follows the Arduino sketch: the screen is portrait, the text area has a margin of
//! 35 px at the top and 10 px on each side, the book list shows 5 books to a screen with a 5 px
//! selection box, and the table of contents 6 entries with a 3 px box. The font is a Times style
//! serif (`u8g2` has no FreeSerif), regular and bold; there is no italic, so italic text is drawn
//! regular.

use alloc::borrow::Cow;
use alloc::string::String;

use embedded_graphics::{
    pixelcolor::Gray4,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use u8g2_fonts::{
    FontRenderer, fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
};

use crate::layout::{Draw, Layout, TextMetrics, wrap_text};

/// Books shown on one screen of the list.
pub const BOOKS_PER_SCREEN: usize = 5;
/// Entries shown on one screen of the table of contents.
pub const ENTRIES_PER_SCREEN: usize = 6;

const LIST_PADDING: u32 = 20;
const LIST_BOX: u32 = 5;
const CONTENTS_PADDING: u32 = 14;
const CONTENTS_BOX: u32 = 3;
/// Indent of a table of contents entry for each level of nesting, and the deepest level indented.
const LEVEL_INDENT: u32 = 14;
const MAX_INDENT_LEVEL: u8 = 3;

fn regular() -> FontRenderer {
    FontRenderer::new::<fonts::u8g2_font_timR18_tf>()
}

fn bold() -> FontRenderer {
    FontRenderer::new::<fonts::u8g2_font_timB18_tf>()
}

/// Where the text goes on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    /// Size of the screen in pixels, as the draw target sees it.
    pub width: u32,
    pub height: u32,
    pub margin_top: u32,
    pub margin_left: u32,
    pub margin_right: u32,
}

impl Geometry {
    /// The PaperDIY held upright: 540 x 960, with the margins of the Arduino sketch.
    pub const PORTRAIT: Self = Self {
        width: 540,
        height: 960,
        margin_top: 35,
        margin_left: 10,
        margin_right: 10,
    };

    /// Width of the text area, the width a line of a page is laid out for.
    pub fn page_width(&self) -> u32 {
        self.width
            .saturating_sub(self.margin_left + self.margin_right)
    }

    /// Height of the text area, the height of a page.
    pub fn page_height(&self) -> u32 {
        self.height.saturating_sub(self.margin_top)
    }
}

/// Replaces what the font has no glyph for: typographic quotes, dashes, the ellipsis and the like
/// become their plain equivalents, soft hyphens disappear, and anything else outside Latin-1
/// becomes `?`. Measuring and drawing both go through this, so they agree.
pub fn glyph_text(text: &str) -> Cow<'_, str> {
    if text
        .chars()
        .all(|c| (c as u32) <= 0xFF && c != '\u{a0}' && c != '\u{ad}')
    {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' | '\u{2032}' => out.push('\''),
            '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{2033}' => out.push('"'),
            '\u{2010}'..='\u{2015}' | '\u{2212}' => out.push('-'),
            '\u{2026}' => out.push_str("..."),
            '\u{2022}' | '\u{2023}' | '\u{25cf}' => out.push('*'),
            '\u{2122}' => out.push_str("TM"),
            '\u{2192}' => out.push_str("->"),
            '\u{fb01}' => out.push_str("fi"),
            '\u{fb02}' => out.push_str("fl"),
            '\u{a0}' | '\u{2002}'..='\u{200a}' | '\u{202f}' => out.push(' '),
            '\u{ad}' | '\u{200b}'..='\u{200d}' | '\u{feff}' => {}
            c if (c as u32) <= 0xFF => out.push(c),
            _ => out.push('?'),
        }
    }
    Cow::Owned(out)
}

/// The font of the reader, as the layout needs to know it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Serif;

impl TextMetrics for Serif {
    fn text_width(&self, text: &str, bold_text: bool, _italic: bool) -> u32 {
        let font = if bold_text { bold() } else { regular() };
        let text = glyph_text(text);
        match font.get_rendered_dimensions(&*text, Point::zero(), VerticalPosition::Top) {
            Ok(dimensions) => dimensions.advance.x.max(0) as u32,
            // Cannot happen after `glyph_text`, but a width is better than a stop.
            Err(_) => text.chars().count() as u32 * 8,
        }
    }

    fn space_width(&self) -> u32 {
        self.text_width(" ", false, false)
    }

    fn line_height(&self) -> u32 {
        regular().get_default_line_height()
    }
}

/// Draws `text` with its top left corner at `(x, y)`.
fn text_at<D: DrawTarget<Color = Gray4>>(
    target: &mut D,
    x: i32,
    y: i32,
    text: &str,
    bold_text: bool,
) {
    let font = if bold_text { bold() } else { regular() };
    let text = glyph_text(text);
    let _ = font.render_aligned(
        &*text,
        Point::new(x, y),
        VerticalPosition::Top,
        HorizontalAlignment::Left,
        FontColor::Transparent(Gray4::BLACK),
        target,
    );
}

/// A frame `thickness` pixels thick, made of nested rectangles as the Arduino sketch does.
fn frame<D: DrawTarget<Color = Gray4>>(target: &mut D, area: Rectangle, thickness: u32) {
    for i in 0..thickness {
        let inset = i as i32;
        let size = Size::new(
            area.size.width.saturating_sub(2 * i),
            area.size.height.saturating_sub(2 * i),
        );
        if size.width == 0 || size.height == 0 {
            break;
        }
        let _ = Rectangle::new(area.top_left + Point::new(inset, inset), size)
            .into_styled(PrimitiveStyle::with_stroke(Gray4::BLACK, 1))
            .draw(target);
    }
}

/// Draws page `index` of `layout`.
pub fn draw_page<D: DrawTarget<Color = Gray4>>(
    target: &mut D,
    geometry: &Geometry,
    layout: &Layout,
    index: usize,
) {
    let (left, top) = (geometry.margin_left as i32, geometry.margin_top as i32);
    layout.draw_page(index, &mut |item| {
        if let Draw::Word {
            x, y, text, bold, ..
        } = item
        {
            text_at(target, left + x, top + y, text, bold);
        }
        // Images are drawn by the shell, which can decode them.
    });
}

/// The screen of the list: `titles` of the books, the one at `selected` boxed, `BOOKS_PER_SCREEN`
/// to a screen, starting at the screen that holds `selected`.
pub fn draw_book_list<D: DrawTarget<Color = Gray4>>(
    target: &mut D,
    geometry: &Geometry,
    titles: &[&str],
    selected: usize,
) {
    draw_items(
        target,
        geometry,
        titles.iter().map(|title| (*title, 0u8)),
        titles.len(),
        selected,
        BOOKS_PER_SCREEN,
        LIST_PADDING,
        LIST_BOX,
    );
}

/// The screen of the table of contents: `(title, level)` of the entries, the one at `selected`
/// boxed, `ENTRIES_PER_SCREEN` to a screen.
pub fn draw_contents<'a, D: DrawTarget<Color = Gray4>>(
    target: &mut D,
    geometry: &Geometry,
    entries: impl ExactSizeIterator<Item = (&'a str, u8)>,
    selected: usize,
) {
    let count = entries.len();
    draw_items(
        target,
        geometry,
        entries,
        count,
        selected,
        ENTRIES_PER_SCREEN,
        CONTENTS_PADDING,
        CONTENTS_BOX,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_items<'a, D: DrawTarget<Color = Gray4>>(
    target: &mut D,
    geometry: &Geometry,
    items: impl Iterator<Item = (&'a str, u8)>,
    count: usize,
    selected: usize,
    per_screen: usize,
    padding: u32,
    box_width: u32,
) {
    let range = crate::reader::items_on_page(selected, per_screen, count);
    let cell = geometry.page_height() / per_screen as u32;
    let metrics = Serif;
    let line = metrics.line_height();
    let text_height = cell.saturating_sub(padding * 2).max(line);
    let (left, top) = (geometry.margin_left as i32, geometry.margin_top as i32);
    for (slot, (index, (title, level))) in items
        .enumerate()
        .skip(range.start)
        .take(range.len())
        .enumerate()
    {
        let y = top + (slot as u32 * cell) as i32;
        let indent = u32::from(level.min(MAX_INDENT_LEVEL)) * LEVEL_INDENT;
        let x = left + (padding / 2 + indent) as i32;
        let lines = wrap_text(
            title,
            &metrics,
            geometry.page_width().saturating_sub(padding + indent),
        );
        // Centered in the cell, and never more lines than fit.
        let used = (lines.len() as u32 * line).min(text_height);
        let mut line_y = y + (cell.saturating_sub(used) / 2) as i32;
        for text in lines.iter().take((text_height / line).max(1) as usize) {
            text_at(target, x, line_y, text, false);
            line_y += line as i32;
        }
        if index == selected {
            let area = Rectangle::new(
                Point::new(left, y + (padding / 2) as i32),
                Size::new(geometry.page_width(), cell.saturating_sub(padding)),
            );
            frame(target, area, box_width);
        }
    }
}

/// A box in the middle of the screen with one or two lines of bold text.
pub fn draw_notice<D: DrawTarget<Color = Gray4>>(
    target: &mut D,
    geometry: &Geometry,
    lines: &[&str],
) {
    let line = Serif.line_height();
    let height = lines.len() as u32 * line + 40;
    let width = lines
        .iter()
        .map(|l| Serif.text_width(l, true, false))
        .max()
        .unwrap_or(0)
        + 60;
    let width = width.min(geometry.width);
    let x = (geometry.width.saturating_sub(width) / 2) as i32;
    let y = (geometry.height.saturating_sub(height) / 2) as i32;
    let area = Rectangle::new(Point::new(x, y), Size::new(width, height));
    let _ = area
        .into_styled(PrimitiveStyle::with_fill(Gray4::WHITE))
        .draw(target);
    frame(target, area, 2);
    for (i, text) in lines.iter().enumerate() {
        text_at(
            target,
            x + 30,
            y + 20 + (i as u32 * line) as i32,
            text,
            true,
        );
    }
}
