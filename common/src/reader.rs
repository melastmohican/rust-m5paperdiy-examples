//! Which screen the reader shows, and what the two buttons do on it. This is the `handleEpubList`,
//! `handleEpubTableContents` and `handleEpub` of the Arduino sketch, without the drawing and the
//! file access: those go through the [`Library`] trait, so the host tests drive it with fake books
//! and the board plugs in the microSD card.
//!
//! The buttons give three actions: [`Action::Down`] (button A click) is next, [`Action::Up`]
//! (button A hold) is previous, and [`Action::Select`] (button B click) opens the chosen book or
//! chapter, or goes back to the book list from a page.
//!
//! The list and the table of contents wrap around at both ends, like the Arduino reader. Reading
//! stops at the first and the last page and says so, where the Arduino reader ran past the end of
//! the book and showed a notice.

use alloc::string::String;

use crate::render::{BOOKS_PER_SCREEN, ENTRIES_PER_SCREEN};

/// What a button press means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Previous book, entry or page.
    Up,
    /// Next book, entry or page.
    Down,
    /// Open the book or chapter, or go back to the list.
    Select,
    /// Forward a screen: to the first item of the next screen of the list or of the contents, round
    /// to the first screen after the last, and [`SKIP_PAGES`] pages ahead while reading.
    Skip,
}

/// How many pages [`Action::Skip`] moves while reading.
pub const SKIP_PAGES: usize = 10;

/// The screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// The books on the card, one chosen.
    List,
    /// The table of contents of the chosen book, one entry chosen.
    Contents,
    /// A page of the book.
    Reading,
}

/// What the screen shows after a press, for the shell to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Nothing changed.
    None,
    /// Draw the screen again: [`App::screen`] says which, and the other getters where.
    Redraw,
    /// A move back from the first page of the book. The position stays.
    StartOfBook,
    /// A move on from the last page of the book. The position stays.
    EndOfBook,
    /// The chosen book could not be opened. The list stays.
    OpenFailed,
}

/// How big an opened book is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BookShape {
    /// Entries in the table of contents.
    pub toc_len: usize,
    /// Documents in the reading order.
    pub spine_len: usize,
}

/// What the reader asks of the world: the books, and the layout of the open one.
pub trait Library {
    /// Number of books on the card.
    fn book_count(&self) -> usize;
    /// Opens book `index` and says how big it is, `None` when it cannot be read.
    fn open(&mut self, index: usize) -> Option<BookShape>;
    /// The spine document table of contents entry `toc` points at, when it points at one.
    fn toc_spine_index(&self, toc: usize) -> Option<usize>;
    /// Number of pages of spine document `section`, laying it out when that has not been done. A
    /// document that cannot be read has none.
    fn pages_in_section(&mut self, section: usize) -> usize;
    /// The part after `#` of the link of table of contents entry `toc`, when it has one. Many books
    /// put all their chapters in one document and tell them apart by this.
    fn toc_fragment(&self, toc: usize) -> Option<String> {
        let _ = toc;
        None
    }
    /// The page of spine document `section` that holds the anchor `fragment`, `None` when the
    /// document has no such anchor.
    fn page_of_anchor(&mut self, section: usize, fragment: &str) -> Option<usize> {
        let _ = (section, fragment);
        None
    }
    /// Lets go of the open book.
    fn close(&mut self);
}

/// The position of the reader: the screen, the chosen book and entry, and the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    screen: Screen,
    book: usize,
    shape: BookShape,
    toc: usize,
    section: usize,
    page: usize,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// Starts on the book list, with the first book chosen.
    pub fn new() -> Self {
        Self {
            screen: Screen::List,
            book: 0,
            shape: BookShape::default(),
            toc: 0,
            section: 0,
            page: 0,
        }
    }

    /// The screen to show.
    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// The chosen book, an index into the list.
    pub fn selected_book(&self) -> usize {
        self.book
    }

    /// The chosen entry of the table of contents.
    pub fn selected_toc(&self) -> usize {
        self.toc
    }

    /// The spine document being read.
    pub fn section(&self) -> usize {
        self.section
    }

    /// The page of that document, from 0.
    pub fn page(&self) -> usize {
        self.page
    }

    /// The size of the open book.
    pub fn shape(&self) -> BookShape {
        self.shape
    }

    /// Applies a press of a button.
    pub fn press(&mut self, action: Action, library: &mut dyn Library) -> Effect {
        match self.screen {
            Screen::List => self.press_list(action, library),
            Screen::Contents => self.press_contents(action, library),
            Screen::Reading => self.press_reading(action, library),
        }
    }

    fn press_list(&mut self, action: Action, library: &mut dyn Library) -> Effect {
        let count = library.book_count();
        if count == 0 {
            return Effect::None;
        }
        // The list may have got shorter since the last press.
        self.book = self.book.min(count - 1);
        match action {
            Action::Up | Action::Down => {
                self.book = step(self.book, count, action);
                Effect::Redraw
            }
            Action::Skip => {
                self.book = next_screen(self.book, BOOKS_PER_SCREEN, count);
                Effect::Redraw
            }
            Action::Select => {
                let Some(shape) = library.open(self.book) else {
                    return Effect::OpenFailed;
                };
                self.shape = shape;
                self.toc = 0;
                self.section = 0;
                self.page = 0;
                // A book without a table of contents is read from its first page.
                self.screen = if shape.toc_len == 0 {
                    Screen::Reading
                } else {
                    Screen::Contents
                };
                Effect::Redraw
            }
        }
    }

    fn press_contents(&mut self, action: Action, library: &mut dyn Library) -> Effect {
        match action {
            Action::Up | Action::Down => {
                self.toc = step(self.toc, self.shape.toc_len, action);
                Effect::Redraw
            }
            Action::Skip => {
                self.toc = next_screen(self.toc, ENTRIES_PER_SCREEN, self.shape.toc_len);
                Effect::Redraw
            }
            Action::Select => {
                self.section = library
                    .toc_spine_index(self.toc)
                    .filter(|&section| section < self.shape.spine_len)
                    .unwrap_or(0);
                // Start on the page that holds the chapter, not on the first page of its document.
                self.page = library
                    .toc_fragment(self.toc)
                    .and_then(|fragment| library.page_of_anchor(self.section, &fragment))
                    .unwrap_or(0);
                self.screen = Screen::Reading;
                Effect::Redraw
            }
        }
    }

    fn press_reading(&mut self, action: Action, library: &mut dyn Library) -> Effect {
        match action {
            Action::Select => {
                library.close();
                self.screen = Screen::List;
                Effect::Redraw
            }
            Action::Down => {
                if self.page + 1 < library.pages_in_section(self.section) {
                    self.page += 1;
                    return Effect::Redraw;
                }
                // On to the next document that has a page.
                for section in self.section + 1..self.shape.spine_len {
                    if library.pages_in_section(section) > 0 {
                        self.section = section;
                        self.page = 0;
                        return Effect::Redraw;
                    }
                }
                Effect::EndOfBook
            }
            Action::Skip => {
                // Ten pages on, as far as there are. Moving at all is a redraw; with no page left it
                // is the end of the book.
                let mut moved = false;
                for _ in 0..SKIP_PAGES {
                    if self.press_reading(Action::Down, library) != Effect::Redraw {
                        break;
                    }
                    moved = true;
                }
                if moved {
                    Effect::Redraw
                } else {
                    Effect::EndOfBook
                }
            }
            Action::Up => {
                if self.page > 0 {
                    self.page -= 1;
                    return Effect::Redraw;
                }
                // Back to the last page of the previous document that has one.
                for section in (0..self.section).rev() {
                    let pages = library.pages_in_section(section);
                    if pages > 0 {
                        self.section = section;
                        self.page = pages - 1;
                        return Effect::Redraw;
                    }
                }
                Effect::StartOfBook
            }
        }
    }
}

/// One step up or down in a list of `count` items, wrapping around.
fn step(index: usize, count: usize, action: Action) -> usize {
    if count == 0 {
        return 0;
    }
    match action {
        Action::Down => (index + 1) % count,
        _ => (index + count - 1) % count,
    }
}

/// The first item of the screen after the one that holds `selected`, for `count` items shown
/// `per_page` to a screen, or the first item of all after the last screen.
fn next_screen(selected: usize, per_page: usize, count: usize) -> usize {
    let next = (selected / per_page + 1) * per_page;
    if next < count { next } else { 0 }
}

/// The indexes of the items on the screen that shows item `selected`, for a list of `count` items
/// shown `per_page` to a screen. The screen changes when the chosen item moves to another one.
pub fn items_on_page(selected: usize, per_page: usize, count: usize) -> core::ops::Range<usize> {
    if per_page == 0 {
        return 0..0;
    }
    let start = selected / per_page * per_page;
    start.min(count)..(start + per_page).min(count)
}
