//! The books on a FAT card: a read-only [`Storage`] over `embedded-sdmmc`, the scan of the card for
//! `.epub` files, and the [`Library`] the reader runs on. Nothing here touches the hardware: the
//! block device is a type parameter, the microSD card on the board and a block of memory in the tests.
//!
//! Books are found by their long name, since the 8.3 name of `Gulliver's Travels.epub` is
//! `GULLI~40.EPU`. A file is opened by its long name, or by its short name when it has none.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::ops::ControlFlow;

use embedded_graphics::{pixelcolor::Gray4, prelude::*};
use embedded_sdmmc::{
    BlockDevice, LfnBuffer, Mode, RawDirectory, RawFile, RawVolume, TimeSource, VolumeIdx,
    VolumeManager,
};
use paperpaw_platform::storage::{DirEntry, Storage, StorageError};

use crate::epub::{Book, dir_of};
use crate::html::{Block, parse_with_anchors};
use crate::image;
use crate::layout::{Draw, Layout, TextMetrics};
use crate::reader::{Action, App, BookShape, Effect, Library, Screen};
use crate::render::{Geometry, Serif, draw_book_list, draw_contents, draw_notice, draw_page};

/// The most books listed, as in the Arduino reader.
pub const MAX_BOOKS: usize = 20;

/// Why the card could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdError {
    /// There is no FAT volume, or the card did not answer.
    Volume,
    /// The root directory could not be read.
    Directory,
}

/// A file of the root directory on a card, read only. One file is kept open, which is what reading a
/// book needs: the zip reader asks for many small pieces of the same file.
pub struct SdStorage<D: BlockDevice, T: TimeSource> {
    manager: VolumeManager<D, T>,
    volume: RawVolume,
    root: RawDirectory,
    open: RefCell<Option<(String, RawFile)>>,
}

impl<D: BlockDevice, T: TimeSource> SdStorage<D, T> {
    /// Opens the first volume of the card and its root directory.
    pub fn new(device: D, time: T) -> Result<Self, SdError> {
        let manager = VolumeManager::new(device, time);
        let volume = manager
            .open_raw_volume(VolumeIdx(0))
            .map_err(|_| SdError::Volume)?;
        let root = manager
            .open_root_dir(volume)
            .map_err(|_| SdError::Directory)?;
        Ok(Self {
            manager,
            volume,
            root,
            open: RefCell::new(None),
        })
    }

    /// Runs `f` on the open file for `path`, opening it first when another one is open.
    fn with_file<R>(
        &self,
        path: &str,
        f: impl FnOnce(&VolumeManager<D, T>, RawFile) -> Result<R, embedded_sdmmc::Error<D::Error>>,
    ) -> Result<R, StorageError> {
        let mut open = self.open.borrow_mut();
        if open.as_ref().is_none_or(|(name, _)| name != path) {
            if let Some((_, file)) = open.take() {
                let _ = self.manager.close_file(file);
            }
            let file = self
                .manager
                .open_long_name_file_in_dir(self.root, path, Mode::ReadOnly)
                .or_else(|_| {
                    self.manager
                        .open_file_in_dir(self.root, path, Mode::ReadOnly)
                })
                .map_err(|_| StorageError::NotFound)?;
            *open = Some((String::from(path), file));
        }
        let Some((_, file)) = open.as_ref() else {
            return Err(StorageError::NotFound);
        };
        f(&self.manager, *file).map_err(|_| StorageError::Io)
    }

    /// The `.epub` files in the root directory, by long name, in the order they are stored. Hidden
    /// files, directories and names starting with `.` (the `._book.epub` files a Mac leaves) are left
    /// out. At most [`MAX_BOOKS`].
    pub fn epub_files(&self) -> Result<Vec<String>, SdError> {
        let mut names = Vec::new();
        let mut buffer = [0u8; 255 * 3];
        let mut lfn = LfnBuffer::new(&mut buffer);
        self.manager
            .iterate_dir_lfn(self.root, &mut lfn, |entry, long| {
                if entry.attributes.is_directory()
                    || entry.attributes.is_hidden()
                    || entry.attributes.is_volume()
                {
                    return ControlFlow::Continue(());
                }
                if let Some(name) = long.filter(|name| is_epub_name(name)) {
                    names.push(String::from(name));
                }
                if names.len() >= MAX_BOOKS {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            })
            .map_err(|_| SdError::Directory)?;
        Ok(names)
    }
}

impl<D: BlockDevice, T: TimeSource> Drop for SdStorage<D, T> {
    fn drop(&mut self) {
        if let Some((_, file)) = self.open.get_mut().take() {
            let _ = self.manager.close_file(file);
        }
        let _ = self.manager.close_dir(self.root);
        let _ = self.manager.close_volume(self.volume);
    }
}

/// True for a file name that ends in `.epub`, in any case, and does not start with `.`.
pub fn is_epub_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() > 5 && bytes[0] != b'.' && bytes[bytes.len() - 5..].eq_ignore_ascii_case(b".epub")
}

impl<D: BlockDevice, T: TimeSource> Storage for SdStorage<D, T> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let offset = u32::try_from(offset).map_err(|_| StorageError::TooLarge)?;
        self.with_file(path, |manager, file| {
            let length = manager.file_length(file)?;
            if offset >= length {
                return Ok(0);
            }
            manager.file_seek_from_start(file, offset)?;
            manager.read(file, buf)
        })
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.with_file(path, |manager, file| manager.file_length(file))
            .map(u64::from)
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

/// A book on the card: the name to open it by, and the title to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookEntry {
    pub path: String,
    pub title: String,
}

/// The open book, and the pages of its documents as far as they are known.
struct Open {
    book: Book,
    /// Number of pages of each spine document, known once it has been laid out.
    pages: Vec<Option<usize>>,
    /// The size of each picture looked at so far, by its path in the archive; `None` for one that is
    /// not in the book or cannot be read.
    sizes: BTreeMap<String, Option<(u32, u32)>>,
}

/// The books in a [`Storage`], and the one being read. The storage is the SD card on the board; the
/// tests use memory or files.
pub struct BookLibrary<S: Storage> {
    storage: S,
    books: Vec<BookEntry>,
    geometry: Geometry,
    open: Option<Open>,
    /// The last document laid out, for drawing its pages.
    layout: Option<(usize, Layout)>,
    /// What the last page drawn did with each of its pictures, for the shell's log.
    pictures: Vec<(String, PictureStatus)>,
}

/// What became of a picture on the page that was drawn last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureStatus {
    /// Read, decoded and drawn; `bytes` is the size of the file.
    Drawn { bytes: usize },
    /// The book has no such file or it could not be read.
    Unreadable,
    /// The file was read but is not a picture that can be decoded, or is too big for the memory.
    NotDecoded { bytes: usize },
}

/// The books on the SD card.
pub type SdLibrary<D, T> = BookLibrary<SdStorage<D, T>>;

impl<D: BlockDevice, T: TimeSource> BookLibrary<SdStorage<D, T>> {
    /// Scans the card for books and reads their titles. A book that cannot be read is left out, the
    /// others are sorted by title.
    pub fn new(storage: SdStorage<D, T>, geometry: Geometry) -> Result<Self, SdError> {
        let files = storage.epub_files()?;
        let mut books: Vec<BookEntry> = files
            .into_iter()
            .filter_map(|path| {
                let book = Book::open(&storage, &path).ok()?;
                let title = if book.title().is_empty() {
                    // No title in the book: the file name without `.epub`.
                    String::from(&path[..path.len() - 5])
                } else {
                    String::from(book.title())
                };
                Some(BookEntry { path, title })
            })
            .collect();
        books.sort_by(|a, b| a.title.cmp(&b.title));
        Ok(Self {
            storage,
            books,
            geometry,
            open: None,
            layout: None,
            pictures: Vec::new(),
        })
    }
}

impl<S: Storage> BookLibrary<S> {
    /// A library of the given books, without scanning the card. For tests, and for a shell that
    /// already knows the books.
    pub fn with_books(storage: S, books: Vec<BookEntry>, geometry: Geometry) -> Self {
        Self {
            storage,
            books,
            geometry,
            open: None,
            layout: None,
            pictures: Vec::new(),
        }
    }

    /// The books, in the order of the list.
    pub fn books(&self) -> &[BookEntry] {
        &self.books
    }

    /// The path of every file inside book `index`, for example to see what images it has.
    pub fn files(&self, index: usize) -> Option<Vec<String>> {
        let entry = self.books.get(index)?;
        let book = Book::open(&self.storage, &entry.path).ok()?;
        book.files(&self.storage).ok()
    }

    /// The table of contents entries of the open book, with where each one points.
    pub fn toc_entries(&self) -> &[crate::epub::TocEntry] {
        self.open.as_ref().map_or(&[], |open| open.book.toc())
    }

    /// The table of contents of the open book as `(title, level)`.
    pub fn contents(&self) -> impl ExactSizeIterator<Item = (&str, u8)> {
        let toc = self.open.as_ref().map_or(&[][..], |open| open.book.toc());
        toc.iter().map(|entry| (entry.title.as_str(), entry.level))
    }

    /// Lays out spine document `section` for drawing, unless it is the one already laid out.
    fn lay_out(&mut self, section: usize) -> Option<&Layout> {
        if self.layout.as_ref().is_none_or(|(s, _)| *s != section) {
            self.layout = None;
            let open = self.open.as_mut()?;
            let data = open.book.read_spine(&self.storage, section).ok()?;
            let base = open.book.spine().get(section).map_or("", |p| dir_of(p));
            let (blocks, anchors) = parse_with_anchors(&data, base);
            // The size of each picture, read from the book once: the layout asks for it by path.
            for block in &blocks {
                if let Block::Image(image) = block
                    && !open.sizes.contains_key(&image.path)
                {
                    let size = open
                        .book
                        .read_path(&self.storage, &image.path)
                        .ok()
                        .and_then(|bytes| image::dimensions(&bytes));
                    open.sizes.insert(image.path.clone(), size);
                }
            }
            let sizes = &open.sizes;
            let layout = Layout::with_anchors(
                blocks,
                anchors,
                &Serif,
                self.geometry.page_width(),
                self.geometry.page_height(),
                &|path| sizes.get(path).copied().flatten(),
            );
            if let Some(slot) = open.pages.get_mut(section) {
                *slot = Some(layout.page_count());
            }
            self.layout = Some((section, layout));
        }
        self.layout.as_ref().map(|(_, layout)| layout)
    }

    /// Draws the screen `app` is on into `target`, which is cleared first by the caller. `effect` is
    /// what the last press did, so a notice can be drawn on top of the page.
    pub fn draw<G: DrawTarget<Color = Gray4>>(
        &mut self,
        app: &App,
        effect: Effect,
        target: &mut G,
    ) {
        let geometry = self.geometry;
        self.pictures.clear();
        match app.screen() {
            Screen::List => {
                let titles: Vec<&str> = self.books.iter().map(|b| b.title.as_str()).collect();
                if titles.is_empty() {
                    draw_notice(
                        target,
                        &geometry,
                        &["No books on the card", "Copy .epub files to it"],
                    );
                } else {
                    draw_book_list(target, &geometry, &titles, app.selected_book());
                }
                if effect == Effect::OpenFailed {
                    draw_notice(target, &geometry, &["Could not open the book"]);
                }
            }
            Screen::Contents => {
                draw_contents(target, &geometry, self.contents(), app.selected_toc());
            }
            Screen::Reading => {
                let mut pictures = Vec::new();
                if let Some(layout) = self.lay_out(app.section()) {
                    draw_page(target, &geometry, layout, app.page());
                    layout.draw_page(app.page(), &mut |item| {
                        if let Draw::Image {
                            path,
                            x,
                            y,
                            width,
                            height,
                        } = item
                        {
                            pictures.push((String::from(path), x, y, width, height));
                        }
                    });
                }
                // The pictures are read and decoded one at a time, so only one is in memory.
                for (path, x, y, width, height) in pictures {
                    let Some(open) = self.open.as_ref() else {
                        break;
                    };
                    let status = match open.book.read_path(&self.storage, &path) {
                        Err(_) => PictureStatus::Unreadable,
                        Ok(bytes) => {
                            let size = bytes.len();
                            match image::decode(&bytes) {
                                Some(gray) => {
                                    drop(bytes);
                                    let left = geometry.margin_left as i32 + x;
                                    let top = geometry.margin_top as i32 + y;
                                    image::draw(target, left, top, width, height, &gray);
                                    PictureStatus::Drawn { bytes: size }
                                }
                                None => PictureStatus::NotDecoded { bytes: size },
                            }
                        }
                    };
                    self.pictures.push((path, status));
                }
                match effect {
                    Effect::EndOfBook => draw_notice(
                        target,
                        &geometry,
                        &["Reached the end of the book", "Use the SELECT button"],
                    ),
                    Effect::StartOfBook => draw_notice(
                        target,
                        &geometry,
                        &["Reached the start of the book", "Use the SELECT button"],
                    ),
                    _ => {}
                }
            }
        }
    }

    /// The words of the page `app` is on, in reading order, as one string. For logs and tests.
    pub fn page_text(&mut self, app: &App) -> Option<String> {
        let page = app.page();
        let layout = self.lay_out(app.section())?;
        let mut text = String::new();
        layout.draw_page(page, &mut |item| {
            if let Draw::Word { text: word, .. } = item {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(word);
            }
        });
        Some(text)
    }

    /// What the last page drawn did with each of its pictures.
    pub fn last_pictures(&self) -> &[(String, PictureStatus)] {
        &self.pictures
    }

    /// The paths of the pictures on the page `app` is on. For logs and tests.
    pub fn pictures_on_page(&mut self, app: &App) -> Vec<String> {
        let page = app.page();
        let mut paths = Vec::new();
        if let Some(layout) = self.lay_out(app.section()) {
            layout.draw_page(page, &mut |item| {
                if let Draw::Image { path, .. } = item {
                    paths.push(String::from(path));
                }
            });
        }
        paths
    }

    /// The line height of the font, for the shell.
    pub fn line_height(&self) -> u32 {
        Serif.line_height()
    }

    /// Applies a button press to `app` and draws the screen it ends on into `target`. Returns what
    /// the press did.
    pub fn press<G: DrawTarget<Color = Gray4>>(
        &mut self,
        app: &mut App,
        action: Action,
        target: &mut G,
    ) -> Effect {
        let effect = app.press(action, self);
        if effect != Effect::None {
            let _ = target.clear(Gray4::WHITE);
            self.draw(app, effect, target);
        }
        effect
    }
}

impl<S: Storage> Library for BookLibrary<S> {
    fn book_count(&self) -> usize {
        self.books.len()
    }

    fn open(&mut self, index: usize) -> Option<BookShape> {
        self.layout = None;
        self.open = None;
        let entry = self.books.get(index)?;
        let book = Book::open(&self.storage, &entry.path).ok()?;
        let shape = BookShape {
            toc_len: book.toc().len(),
            spine_len: book.spine().len(),
        };
        self.open = Some(Open {
            pages: alloc::vec![None; shape.spine_len],
            sizes: BTreeMap::new(),
            book,
        });
        Some(shape)
    }

    fn toc_spine_index(&self, toc: usize) -> Option<usize> {
        self.open.as_ref()?.book.toc().get(toc)?.spine_index
    }

    fn pages_in_section(&mut self, section: usize) -> usize {
        if let Some(Some(pages)) = self.open.as_ref().and_then(|o| o.pages.get(section)) {
            return *pages;
        }
        // Lay it out (which keeps the page count), or it cannot be read and has no pages.
        let pages = self.lay_out(section).map_or(0, Layout::page_count);
        if let Some(slot) = self.open.as_mut().and_then(|o| o.pages.get_mut(section)) {
            slot.get_or_insert(pages);
        }
        pages
    }

    fn toc_fragment(&self, toc: usize) -> Option<String> {
        self.open.as_ref()?.book.toc().get(toc)?.fragment.clone()
    }

    fn page_of_anchor(&mut self, section: usize, fragment: &str) -> Option<usize> {
        self.lay_out(section)?.page_of_anchor(fragment)
    }

    fn close(&mut self) {
        self.layout = None;
        self.open = None;
    }
}
