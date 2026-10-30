//! Tests of `common/src/reader.rs`: the screens of the reader and what the buttons do on each,
//! driven with fake books, and with real ones where `EPUB_TEST_FILES` names them.

use m5paperdiy_common::reader::{Action, App, BookShape, Effect, Library, Screen, items_on_page};

use Action::{Down, Select, Up};

/// A book for the tests: where each table of contents entry points, and the pages of each document.
#[derive(Clone)]
struct FakeBook {
    openable: bool,
    toc: Vec<Option<usize>>,
    pages: Vec<usize>,
}

#[derive(Default)]
struct Fake {
    books: Vec<FakeBook>,
    open: Option<usize>,
    closed: usize,
}

impl Fake {
    fn with(books: Vec<FakeBook>) -> Self {
        Self {
            books,
            ..Self::default()
        }
    }

    fn open_book(&self) -> &FakeBook {
        &self.books[self.open.expect("a book is open")]
    }
}

fn book(toc: &[Option<usize>], pages: &[usize]) -> FakeBook {
    FakeBook {
        openable: true,
        toc: toc.to_vec(),
        pages: pages.to_vec(),
    }
}

impl Library for Fake {
    fn book_count(&self) -> usize {
        self.books.len()
    }
    fn open(&mut self, index: usize) -> Option<BookShape> {
        let b = self.books.get(index).filter(|b| b.openable)?;
        let shape = BookShape {
            toc_len: b.toc.len(),
            spine_len: b.pages.len(),
        };
        self.open = Some(index);
        Some(shape)
    }
    fn toc_spine_index(&self, toc: usize) -> Option<usize> {
        self.open_book().toc.get(toc).copied().flatten()
    }
    fn pages_in_section(&mut self, section: usize) -> usize {
        self.open_book().pages.get(section).copied().unwrap_or(0)
    }
    fn close(&mut self) {
        self.open = None;
        self.closed += 1;
    }
}

fn three_books() -> Fake {
    Fake::with(vec![
        book(&[Some(0)], &[1]),
        // Five documents, the third has no page. Three table of contents entries.
        book(&[Some(0), Some(2), Some(4)], &[2, 3, 0, 1, 4]),
        book(&[], &[2, 2]),
    ])
}

/// Presses the buttons and returns what each press did.
fn press(app: &mut App, lib: &mut Fake, actions: &[Action]) -> Vec<Effect> {
    actions.iter().map(|&a| app.press(a, lib)).collect()
}

fn at(app: &App) -> (Screen, usize, usize) {
    (app.screen(), app.section(), app.page())
}

#[test]
fn the_list_wraps_around_and_select_opens_the_contents() {
    let mut lib = three_books();
    let mut app = App::new();
    assert_eq!((app.screen(), app.selected_book()), (Screen::List, 0));
    press(&mut app, &mut lib, &[Down, Down, Down]);
    assert_eq!(app.selected_book(), 0, "three steps down go round");
    press(&mut app, &mut lib, &[Up]);
    assert_eq!(
        app.selected_book(),
        2,
        "up from the first wraps to the last"
    );
    press(&mut app, &mut lib, &[Down, Down]);
    assert_eq!(app.selected_book(), 1);
    assert_eq!(app.press(Select, &mut lib), Effect::Redraw);
    assert_eq!(app.screen(), Screen::Contents);
    assert_eq!(
        app.shape(),
        BookShape {
            toc_len: 3,
            spine_len: 5
        }
    );
    assert_eq!(app.selected_toc(), 0);
}

#[test]
fn the_contents_wrap_and_select_reads_from_the_chosen_chapter() {
    let mut lib = three_books();
    let mut app = App::new();
    press(&mut app, &mut lib, &[Down, Select]);
    press(&mut app, &mut lib, &[Up]);
    assert_eq!(app.selected_toc(), 2, "up from the first entry wraps");
    press(&mut app, &mut lib, &[Down, Down]);
    assert_eq!(app.selected_toc(), 1);
    assert_eq!(app.press(Select, &mut lib), Effect::Redraw);
    // Entry 1 points at document 2, which has no page: the page there is 0 and the next move goes on.
    assert_eq!(at(&app), (Screen::Reading, 2, 0));
}

#[test]
fn pages_turn_and_cross_documents_both_ways_skipping_empty_ones() {
    let mut lib = three_books();
    let mut app = App::new();
    press(&mut app, &mut lib, &[Down, Select, Select]);
    assert_eq!(at(&app), (Screen::Reading, 0, 0));

    // Forward through every page: (0,0) (0,1) (1,0) (1,1) (1,2) then document 2 is empty, so (3,0)
    // (4,0) (4,1) (4,2) (4,3).
    let mut seen = vec![at(&app)];
    loop {
        match app.press(Down, &mut lib) {
            Effect::Redraw => seen.push(at(&app)),
            Effect::EndOfBook => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    let reading = |s, p| (Screen::Reading, s, p);
    assert_eq!(
        seen,
        [
            reading(0, 0),
            reading(0, 1),
            reading(1, 0),
            reading(1, 1),
            reading(1, 2),
            reading(3, 0),
            reading(4, 0),
            reading(4, 1),
            reading(4, 2),
            reading(4, 3),
        ]
    );
    // At the end the position stays, and pressing again says so again.
    assert_eq!(at(&app), reading(4, 3));
    assert_eq!(app.press(Down, &mut lib), Effect::EndOfBook);
    assert_eq!(at(&app), reading(4, 3));

    // Back the same way.
    let mut back = vec![at(&app)];
    loop {
        match app.press(Up, &mut lib) {
            Effect::Redraw => back.push(at(&app)),
            Effect::StartOfBook => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    back.reverse();
    assert_eq!(back, seen, "going back retraces the pages");
    assert_eq!(at(&app), reading(0, 0));
}

#[test]
fn select_on_a_page_goes_back_to_the_list_with_the_same_book() {
    let mut lib = three_books();
    let mut app = App::new();
    press(&mut app, &mut lib, &[Down, Select, Select, Down]);
    assert_eq!(app.screen(), Screen::Reading);
    assert_eq!(app.press(Select, &mut lib), Effect::Redraw);
    assert_eq!((app.screen(), app.selected_book()), (Screen::List, 1));
    assert_eq!(lib.closed, 1, "the book was let go");
    assert!(lib.open.is_none());
    // And the book can be opened again.
    press(&mut app, &mut lib, &[Select]);
    assert_eq!(app.screen(), Screen::Contents);
}

#[test]
fn a_book_that_cannot_be_opened_leaves_the_list_as_it_was() {
    let mut lib = three_books();
    lib.books[1].openable = false;
    let mut app = App::new();
    press(&mut app, &mut lib, &[Down]);
    assert_eq!(app.press(Select, &mut lib), Effect::OpenFailed);
    assert_eq!((app.screen(), app.selected_book()), (Screen::List, 1));
    // Another book still opens.
    press(&mut app, &mut lib, &[Down]);
    assert_eq!(app.press(Select, &mut lib), Effect::Redraw);
}

#[test]
fn a_book_without_a_table_of_contents_opens_at_its_first_page() {
    let mut lib = three_books();
    let mut app = App::new();
    press(&mut app, &mut lib, &[Up, Select]);
    assert_eq!(app.selected_book(), 2);
    assert_eq!(at(&app), (Screen::Reading, 0, 0));
    assert_eq!(app.press(Down, &mut lib), Effect::Redraw);
    assert_eq!(at(&app), (Screen::Reading, 0, 1));
}

#[test]
fn an_entry_that_points_nowhere_opens_at_the_start() {
    let mut lib = Fake::with(vec![book(&[None, Some(99)], &[2, 2])]);
    let mut app = App::new();
    press(&mut app, &mut lib, &[Select, Select]);
    assert_eq!(at(&app), (Screen::Reading, 0, 0), "no target");
    press(&mut app, &mut lib, &[Select, Select, Down, Select]);
    assert_eq!(
        at(&app),
        (Screen::Reading, 0, 0),
        "a target past the last document"
    );
}

#[test]
fn an_empty_library_does_nothing_and_a_shorter_one_keeps_the_choice_in_range() {
    let mut lib = Fake::default();
    let mut app = App::new();
    assert_eq!(
        press(&mut app, &mut lib, &[Up, Down, Select]),
        [Effect::None; 3]
    );
    assert_eq!(app.screen(), Screen::List);

    let mut lib = three_books();
    let mut app = App::new();
    press(&mut app, &mut lib, &[Up]);
    assert_eq!(app.selected_book(), 2);
    lib.books.truncate(1);
    press(&mut app, &mut lib, &[Down]);
    assert_eq!(app.selected_book(), 0, "the list got shorter");
}

#[test]
fn a_book_with_nothing_to_read_ends_at_once() {
    let mut lib = Fake::with(vec![book(&[Some(0)], &[0, 0])]);
    let mut app = App::new();
    press(&mut app, &mut lib, &[Select, Select]);
    assert_eq!(app.press(Down, &mut lib), Effect::EndOfBook);
    assert_eq!(app.press(Up, &mut lib), Effect::StartOfBook);
    // And a book without any document at all.
    let mut lib = Fake::with(vec![book(&[], &[])]);
    let mut app = App::new();
    press(&mut app, &mut lib, &[Select]);
    assert_eq!(app.press(Down, &mut lib), Effect::EndOfBook);
}

#[test]
fn the_items_on_a_screen_follow_the_chosen_one() {
    assert_eq!(items_on_page(0, 5, 12), 0..5);
    assert_eq!(items_on_page(4, 5, 12), 0..5);
    assert_eq!(items_on_page(5, 5, 12), 5..10);
    assert_eq!(items_on_page(11, 5, 12), 10..12, "the last screen is short");
    assert_eq!(items_on_page(0, 5, 0), 0..0);
    assert_eq!(items_on_page(3, 0, 12), 0..0);
}

fn xorshift(mut state: u32) -> impl FnMut() -> u32 {
    move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    }
}

#[test]
fn random_presses_on_random_libraries_never_panic_and_stay_in_range() {
    let mut random = xorshift(0xC0FF_EE11);
    for round in 0..300 {
        let books: Vec<FakeBook> = (0..random() % 4)
            .map(|_| {
                let spine = (random() % 6) as usize;
                FakeBook {
                    openable: random() % 5 != 0,
                    toc: (0..random() % 5)
                        .map(|_| match random() % 4 {
                            0 => None,
                            _ => Some((random() % (spine as u32 + 2)) as usize),
                        })
                        .collect(),
                    pages: (0..spine).map(|_| (random() % 4) as usize).collect(),
                }
            })
            .collect();
        let mut lib = Fake::with(books);
        let mut app = App::new();
        for step in 0..400 {
            let action = [Up, Down, Select][(random() % 3) as usize];
            app.press(action, &mut lib);
            let ctx = format!("round {round}, step {step}");
            if lib.books.is_empty() {
                assert_eq!(app.screen(), Screen::List, "{ctx}");
                continue;
            }
            assert!(app.selected_book() < lib.books.len(), "{ctx}: chosen book");
            match app.screen() {
                Screen::List => {}
                Screen::Contents => {
                    assert!(
                        app.selected_toc() < app.shape().toc_len,
                        "{ctx}: chosen entry"
                    );
                }
                Screen::Reading => {
                    let shape = app.shape();
                    assert!(
                        app.section() < shape.spine_len || app.section() == 0,
                        "{ctx}: document {} of {}",
                        app.section(),
                        shape.spine_len
                    );
                    let pages = lib.pages_in_section(app.section());
                    assert!(
                        app.page() < pages || app.page() == 0,
                        "{ctx}: page {} of {pages}",
                        app.page()
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------

/// Real books, read from the paths in `EPUB_TEST_FILES` (separated by `:`): open each from the
/// list, choose its first chapter, turn to the last page and back to the first.
#[test]
#[ignore = "needs real books: set EPUB_TEST_FILES"]
fn a_real_book_is_read_from_the_list_to_the_last_page_and_back() {
    use m5paperdiy_common::epub::{Book, dir_of};
    use m5paperdiy_common::html::parse;
    use m5paperdiy_common::layout::{Layout, TextMetrics};
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
    struct Font;
    impl TextMetrics for Font {
        fn text_width(&self, text: &str, _: bool, _: bool) -> u32 {
            text.chars().count() as u32 * 8
        }
        fn space_width(&self) -> u32 {
            4
        }
        fn line_height(&self) -> u32 {
            20
        }
    }

    struct Real {
        paths: Vec<String>,
        open: Option<(Book, Vec<Option<usize>>)>,
    }
    impl Library for Real {
        fn book_count(&self) -> usize {
            self.paths.len()
        }
        fn open(&mut self, index: usize) -> Option<BookShape> {
            let book = Book::open(&Disk, &self.paths[index]).ok()?;
            let shape = BookShape {
                toc_len: book.toc().len(),
                spine_len: book.spine().len(),
            };
            self.open = Some((book, vec![None; shape.spine_len]));
            Some(shape)
        }
        fn toc_spine_index(&self, toc: usize) -> Option<usize> {
            self.open.as_ref()?.0.toc().get(toc)?.spine_index
        }
        fn pages_in_section(&mut self, section: usize) -> usize {
            let Some((book, cache)) = self.open.as_mut() else {
                return 0;
            };
            if let Some(Some(pages)) = cache.get(section) {
                return *pages;
            }
            let pages = book
                .read_spine(&Disk, section)
                .map(|data| {
                    let blocks = parse(&data, dir_of(&book.spine()[section]));
                    Layout::new(blocks, &Font, 480, 800, &|_| None).page_count()
                })
                .unwrap_or(0);
            if let Some(slot) = cache.get_mut(section) {
                *slot = Some(pages);
            }
            pages
        }
        fn close(&mut self) {
            self.open = None;
        }
    }

    let files = std::env::var("EPUB_TEST_FILES").expect("set EPUB_TEST_FILES");
    let paths: Vec<String> = files
        .split(':')
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect();
    let mut lib = Real { paths, open: None };
    let mut app = App::new();
    for index in 0..lib.paths.len() {
        let name = lib.paths[index].clone();
        // The list: move to the book, then open it and choose the first chapter.
        while app.selected_book() != index {
            assert_eq!(app.press(Down, &mut lib), Effect::Redraw);
        }
        assert_eq!(app.press(Select, &mut lib), Effect::Redraw, "{name}: open");
        assert_eq!(app.screen(), Screen::Contents, "{name}");
        assert_eq!(
            app.press(Select, &mut lib),
            Effect::Redraw,
            "{name}: chapter"
        );
        assert_eq!(app.screen(), Screen::Reading, "{name}");

        // Forward to the last page, then back to the first.
        let mut forward = vec![(app.section(), app.page())];
        while app.press(Down, &mut lib) == Effect::Redraw {
            forward.push((app.section(), app.page()));
        }
        let mut back = vec![(app.section(), app.page())];
        while app.press(Up, &mut lib) == Effect::Redraw {
            back.push((app.section(), app.page()));
        }
        back.reverse();
        // Going back goes past where the chapter started, so the forward walk is its tail.
        assert!(
            back.ends_with(&forward),
            "{name}: going back retraces the pages"
        );
        println!(
            "{name}: {} pages from the first chapter, {} in the whole book",
            forward.len(),
            back.len()
        );
        assert!(forward.len() > 1, "{name}: nothing to turn to");

        assert_eq!(app.press(Select, &mut lib), Effect::Redraw);
        assert_eq!(app.screen(), Screen::List, "{name}: back to the list");
    }
}
