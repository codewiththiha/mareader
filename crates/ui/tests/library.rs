//! The library: opens are recorded, positions kept, books reopened.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use mareader_library::{
    BookId, CoverFit, Format, Home, Layout, Library, LinkId, LinkTo, Move, Origin, ShelfId, SortKey,
};
use mareader_settings::Bar;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::library::{
    Action, BookAction, Named, Pick, PickAction, SearchAction, Sheet, ShelfAction,
};

use crate::common::{act, carry, go_to, reader, release};

/// Canonical, as the library records it; opens take any spelling.
fn fixture(name: &str) -> PathBuf {
    long(name).canonicalize().unwrap()
}

/// The fixture by way of `crates/ui/..`.
fn long(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../epub/tests/books");
    dir.join(name)
}

fn raven() -> PathBuf {
    fixture("the-raven-gutenberg-17192.epub")
}

fn harness(app: MareaderApp) -> Harness<'static, MareaderApp> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn open(h: &mut Harness<'static, MareaderApp>, key: &str, value: &str) {
    h.state_mut().set_option(key, value).ok();
    h.run();
}

fn library<'a>(h: &'a Harness<'static, MareaderApp>) -> &'a Library {
    h.state().library()
}

/// The page the focused pane shows, as the library would record it.
fn page(h: &Harness<'static, MareaderApp>) -> usize {
    h.state().last_document().map_or(usize::MAX, |d| d.page)
}

/// What the app holds, for a failing assertion's message.
fn state(h: &Harness<'static, MareaderApp>) -> String {
    let app = h.state();
    let pane = app.active_pane();
    let open = app.documents().reading(pane).map(|r| r.name);
    let toast = app.toast().map(|t| t.message.clone());
    let books: Vec<_> = app.library().books().map(|b| b.locations.clone()).collect();
    format!(
        "open {open:?}, title {}, toast {toast:?}, books {books:?}",
        app.title()
    )
}

#[test]
fn an_opened_book_is_recorded_once_and_reopens_where_it_was_left() {
    let path = raven();
    let mut h = reader(&[("epub", path.to_str().unwrap())]);
    let id = library(&h).book_at(&path);
    let id = id.unwrap_or_else(|| panic!("{path:?} not recorded: {}", state(&h)));
    let book = library(&h).book(id).unwrap();
    assert_eq!(book.format, Format::Epub);
    assert!(!book.title.value.is_empty());
    let pages = h.state().documents().page_count(h.state().active_pane());
    assert!(pages.unwrap_or(0) > 3, "{pages:?}");
    go_to(&mut h, 2);
    assert_eq!(library(&h).position(id).map(|p| p.page), Some(2));
    // Something else, then the book again: page 2, still one book.
    open(&mut h, "pdf", "sample:5");
    assert_eq!(h.state().title(), "A Mareader Sample");
    // Any spelling of the file finds its book.
    let long = long("the-raven-gutenberg-17192.epub");
    open(&mut h, "epub", long.to_str().unwrap());
    assert_eq!(page(&h), 2);
    assert_eq!(library(&h).len(), 1, "samples aren't files, so not books");
}

#[test]
fn text_files_are_recorded_as_themselves() {
    let path = fixture("river.md");
    let h = reader(&[("epub", path.to_str().unwrap())]);
    let id = library(&h).book_at(&path);
    let id = id.unwrap_or_else(|| panic!("{path:?} not recorded: {}", state(&h)));
    assert_eq!(
        library(&h).book(id).map(|b| b.format),
        Some(Format::Markdown)
    );
}

#[test]
fn the_library_page_opens_a_book_where_it_was_left() {
    let path = raven();
    let mut h = reader(&[("epub", path.to_str().unwrap())]);
    go_to(&mut h, 3);
    open(&mut h, "pdf", "sample:5");
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    assert!(h.state().library_showing());
    assert!(h.query_by_label("1 book").is_some(), "{}", state(&h));
    let title = library(&h).recent()[0].title.value.clone();
    h.get_by_label(&title).click();
    h.run();
    assert!(!h.state().library_showing());
    assert_eq!(page(&h), 3);
    assert_eq!(h.state().last_document().map(|d| d.path), Some(path));
}

#[test]
fn escape_leaves_the_library_only_for_something_to_read() {
    let mut h = reader(&[("library", "show")]);
    assert!(h.query_by_label("Your library is empty").is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().library_showing(), "nothing open to go back to");
    open(&mut h, "pdf", "sample:5");
    assert!(!h.state().library_showing(), "an open shows the reader");
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!h.state().library_showing());
}

#[test]
fn a_missing_book_asks_where_it_went_instead_of_opening() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    assert!(h.query_by_label("4 books").is_some());
    let missing = library(&h).books().find(|b| b.missing()).unwrap();
    let title = missing.title.value.clone();
    h.get_by_label(&title).click();
    h.run();
    assert!(h.state().library_showing());
    assert!(h.query_by_label(&format!("Find “{title}”")).is_some());
    assert!(h.query_by_label("Choose the file…").is_some());
    assert!(h.query_by_label("Search a folder…").is_some());
    // Escape closes the sheet, not the library; nothing changed.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("Choose the file…").is_none());
    assert!(h.state().library_showing());
    assert!(library(&h).books().any(|b| b.missing()));
}

#[test]
fn the_library_and_positions_come_back_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    let path = raven();
    let launch = || {
        let mut app = MareaderApp::default();
        app.set_option("workers", "0").ok();
        app.attach_library(root.clone());
        harness(app)
    };
    let mut h = launch();
    open(&mut h, "epub", path.to_str().unwrap());
    go_to(&mut h, 2);
    // Writes leave half a second apart; give them a second.
    h.run_steps(60);
    let id = library(&h).book_at(&path);
    let id = id.unwrap_or_else(|| panic!("{path:?} not recorded: {}", state(&h)));
    assert!(root.join("library.sqlite").is_file());
    drop(h);

    let mut h = launch();
    assert_eq!(library(&h).position(id).map(|p| p.page), Some(2));
    open(&mut h, "epub", path.to_str().unwrap());
    assert_eq!(page(&h), 2);
}

fn top(h: &Harness<'static, MareaderApp>, label: &str) -> f32 {
    h.get_by_label(label).rect().top()
}

#[test]
fn titles_with_numbers_sort_by_their_value() {
    let options = [("books", "10"), ("layout", "list"), ("sort", "title")];
    let h = reader(&[options.as_slice(), &[("library", "show")]].concat());
    let order = [
        "A Field Guide to Ferns",
        "An Atlas of Rivers",
        "Collected Essays, Volume 2",
        "Collected Essays, Volume 10",
    ];
    let tops: Vec<f32> = order.iter().map(|t| top(&h, t)).collect();
    assert!(tops.windows(2).all(|w| w[0] < w[1]), "{tops:?}");
}

#[test]
fn the_library_menu_offers_the_shortcut_sheet() {
    let mut h = reader(&[("library", "show")]);
    h.get_by_label("View options").click();
    h.run();
    h.get_by_label("Keyboard shortcuts").click();
    h.run();
    assert!(mareader_ui::shortcuts::is_open(&h.ctx));
}

#[test]
fn the_view_menu_sets_layout_columns_covers_and_order() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let view = |h: &Harness<'static, MareaderApp>| h.state().settings().library;
    assert_eq!(view(&h).layout, Layout::Grid);
    h.get_by_label("View options").click();
    h.run();
    h.get_by_label("List").click();
    h.run();
    assert_eq!(view(&h).layout, Layout::List);
    h.get_by_label("Grid").click();
    h.run();
    // 900 wide: five automatic columns, so one more pins six.
    h.get_by_label("More columns").click();
    h.run();
    assert_eq!(view(&h).columns, Some(6));
    h.get_by_label("Auto").click();
    h.run();
    assert_eq!(view(&h).columns, None);
    h.get_by_label("Crop").click();
    h.run();
    assert_eq!(view(&h).cover, CoverFit::Crop);
    // Sort by starts shut, keeping only the order in use.
    let shown = |h: &Harness<'static, MareaderApp>, label| h.query_by_label(label).is_some();
    assert!(shown(&h, "Arranged") && !shown(&h, "Title"));
    h.get_by_label("Sort by").click();
    h.run_steps(30);
    h.get_by_label("Title").click();
    h.run();
    assert_eq!((view(&h).sort, view(&h).ascending), (SortKey::Title, true));
    h.get_by_label("Z–A").click();
    h.run();
    assert!(!view(&h).ascending);
    h.get_by_label("Sort by").click();
    h.run_steps(30);
    assert!(
        shown(&h, "Title") && !shown(&h, "Arranged"),
        "shut, it keeps Title"
    );
    assert!(shown(&h, "Z–A"), "and its direction");
}

#[test]
fn a_book_with_a_cover_shows_it_in_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("covered.epub");
    std::fs::write(&path, mareader_epub::sample::epub(2, "Covered")).unwrap();
    let mut h = reader(&[("epub", path.to_str().unwrap())]);
    assert_eq!(h.state().covers_shown(), 0, "none until the page shows");
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    assert!(h.query_by_label("Covered").is_some(), "{}", state(&h));
    assert_eq!(h.state().covers_shown(), 1, "{}", state(&h));
    open(&mut h, "layout", "list");
    assert_eq!(h.state().covers_shown(), 1, "the list reuses it");
}

#[test]
fn the_library_keeps_its_own_title_bar_pin() {
    let mut h = reader(&[("pdf", "sample:5")]);
    let chrome = |h: &Harness<'static, MareaderApp>| {
        let c = &h.state().settings().chrome;
        (c.pinned(Bar::Reader), c.pinned(Bar::Library))
    };
    assert!(!h.state().titlebar().pinned);
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    assert!(h.state().titlebar().pinned, "the library pins by default");
    h.state_mut().queue(Command::SetTitlebarPinned(false));
    h.run();
    assert_eq!(chrome(&h), (false, false));
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    assert!(!h.state().library_showing());
    h.state_mut().queue(Command::SetTitlebarPinned(true));
    h.run();
    assert_eq!(chrome(&h), (true, false), "the reader's pin alone");
    assert!(h.state().titlebar().pinned);
}

#[test]
fn the_library_shows_no_rail_and_only_its_own_bar_tools() {
    let mut h = reader(&[("pdf", "sample:5"), ("sidebar", "outline")]);
    assert!(h.query_by_label("Close sidebar").is_some());
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    // The library's bar starts pinned, so all of it is on show.
    let reader_only = ["Close sidebar", "Toggle sidebar", "Search", "View & tools"];
    for gone in reader_only {
        assert!(h.query_by_label(gone).is_none(), "{gone} in the library");
    }
    for kept in ["Back to reading", "View options", "Appearance", "Settings"] {
        assert!(h.query_by_label(kept).is_some(), "{kept} missing");
    }
    assert!(h.state().sidebar().is_open(), "kept for the way back");
    h.get_by_label("Back to reading").click();
    h.run();
    assert!(!h.state().library_showing());
    assert!(h.query_by_label("Close sidebar").is_some());
}

#[test]
fn the_library_settings_list_only_what_it_uses() {
    use mareader_ui::settings_window::Tab;
    let listed =
        |h: &Harness<'static, MareaderApp>, tab: &str| h.query_all_by_label(tab).count() > 0;
    let mut h = reader(&[("pdf", "sample:5")]);
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    h.get_by_label("Settings").click();
    h.run();
    assert!(h.state().settings_window().is_open());
    assert_eq!(h.state().settings_window().tab(), Tab::ReadAloud);
    for kept in ["Read aloud", "Highlights", "Vocabulary", "AI models"] {
        assert!(listed(&h, kept), "{kept} missing");
    }
    for gone in ["Layout", "Panes", "Motion"] {
        assert!(!listed(&h, gone), "{gone} in the library");
    }
    // Back with the books, every tab.
    h.state_mut().queue(Command::CloseSettings);
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    h.state_mut().queue(Command::ShowSettings);
    h.run();
    for tab in ["Layout", "Panes", "Motion"] {
        assert!(listed(&h, tab), "{tab} missing in the reader");
    }
}

#[test]
fn an_empty_library_offers_no_way_back() {
    let h = reader(&[("library", "show")]);
    assert!(h.query_by_label("Back to reading").is_none());
    assert!(h.query_by_label("View options").is_some());
}

#[test]
fn reader_commands_and_keys_do_nothing_in_the_library() {
    let mut h = reader(&[("pdf", "sample:5")]);
    h.state_mut().queue(Command::ShowFind);
    h.run();
    assert!(h.state().find().is_open());
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    assert!(
        !h.state().find().is_open(),
        "the find bar ends with the panes"
    );
    for c in [
        Command::Split(mareader_workspace::SplitAxis::Horizontal),
        Command::ToggleSidebar,
        Command::LastPage,
    ] {
        h.state_mut().queue(c);
    }
    h.key_press(egui::Key::End);
    h.run();
    assert!(h.state().library_showing());
    assert_eq!(h.state().panes().len(), 1);
    assert!(!h.state().sidebar().is_open());
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    let app = h.state();
    let page = app.documents().current(app.active_pane());
    assert_eq!(page, Some(0), "no page turned behind the library");
}

#[test]
fn the_library_appearance_menu_has_no_page_texture() {
    let mut h = reader(&[("pdf", "sample:5"), ("pinned", "true")]);
    h.get_by_label("Appearance").click();
    h.run();
    assert!(h.query_by_label("Page texture").is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    h.get_by_label("Appearance").click();
    h.run();
    assert!(h.query_by_label("Mode & colour").is_some());
    assert!(
        h.query_by_label("Page texture").is_none(),
        "the shelf has no page"
    );
}

#[test]
fn the_view_menu_key_opens_the_librarys_own_menu() {
    let mut h = reader(&[("library", "show"), ("books", "3")]);
    h.state_mut().queue(Command::ShowViewMenu);
    h.run();
    assert!(h.query_by_label("Sort by").is_some());
    assert!(
        h.query_by_label("Full screen").is_none(),
        "not the reader's"
    );
}

#[test]
fn library_show_after_an_open_lands_in_the_library() {
    let h = reader(&[("pdf", "sample:5"), ("library", "show")]);
    assert!(h.state().library_showing());
    assert!(h.query_by_label("Back to reading").is_some());
}

const NOTES: &[u8] = b"---\ntitle: Field Notes\nauthor: A. Walker\n---\n\n# One\n\nText.\n";

fn raven_bytes() -> Vec<u8> {
    std::fs::read(raven()).unwrap()
}

/// Writes a file an hour old, so no scan waits for it to settle.
fn put(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    let old = SystemTime::now() - Duration::from_secs(3_600);
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(old).unwrap();
    path.canonicalize().unwrap()
}

fn toast(h: &Harness<'static, MareaderApp>) -> String {
    let toast = h.state().toast().map(|t| t.message.clone());
    toast.unwrap_or_default()
}

fn moved(from: &Path, to: &Path) -> PathBuf {
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::rename(from, to).unwrap();
    to.canonicalize().unwrap()
}

#[test]
fn a_folder_brings_in_its_books_where_they_are_and_says_how_many() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Books");
    put(&root, "raven.epub", &raven_bytes());
    put(&root, "notes/notes.md", NOTES);
    put(&root, "letter.docx", b"not a book");
    put(&root, ".trash/old.md", b"# Old");
    let mut h = reader(&[("library", "show")]);
    act(&mut h, read_in(root.clone()));
    assert_eq!(toast(&h), "Added 2 books from “Books”.", "{}", state(&h));
    let titles: Vec<String> = library(&h).books().map(|b| b.title.value.clone()).collect();
    assert!(titles.contains(&"Field Notes".to_owned()), "{titles:?}");
    assert!(titles.iter().any(|t| t.contains("Raven")), "{titles:?}");
    // Found, not opened; nothing was copied or moved.
    assert!(library(&h).books().all(|b| b.opened.value == 0));
    assert!(root.join("raven.epub").is_file());
    let top = library(&h).children(None);
    assert_eq!(top[0].name.value, "Books", "a shelf for the folder");
    // A subfolder of a tree read in place continues it: nothing new.
    act(&mut h, read_in(root.join("notes")));
    nothing_new(&mut h);
    assert_eq!(library(&h).len(), 2);
    assert_eq!(
        library(&h).folders().count(),
        1,
        "one tree, no second folder"
    );
}

#[test]
fn coming_back_to_the_window_finds_new_files_and_notices_gone_ones() {
    let dir = tempfile::tempdir().unwrap();
    let raven = put(dir.path(), "raven.epub", &raven_bytes());
    let mut h = reader(&[("library", "show")]);
    act(&mut h, read_in(dir.path()));
    let first = library(&h).book_at(&raven).unwrap();
    std::fs::remove_file(&raven).unwrap();
    let notes = put(dir.path(), "notes.md", NOTES);
    // Rescans on focus wait out a short gap after the last.
    h.run_steps(150);
    h.input_mut().focused = false;
    h.run();
    h.input_mut().focused = true;
    h.run();
    h.run();
    assert!(library(&h).book_at(&notes).is_some(), "{}", state(&h));
    assert!(library(&h).book(first).unwrap().missing());
}

#[test]
fn a_file_moved_within_a_folder_keeps_its_book_and_place() {
    let dir = tempfile::tempdir().unwrap();
    let raven = put(dir.path(), "new/raven.epub", &raven_bytes());
    let mut h = reader(&[("library", "show")]);
    act(&mut h, read_in(dir.path()));
    let id = library(&h).book_at(&raven).unwrap();
    h.state_mut().queue(Command::OpenBook(id));
    h.run();
    go_to(&mut h, 2);
    let now = moved(&raven, &dir.path().join("read/The Raven.epub"));
    act(&mut h, Action::Rescan);
    assert_eq!(library(&h).book_at(&now), Some(id), "{}", state(&h));
    assert_eq!(library(&h).len(), 1);
    assert_eq!(library(&h).position(id).map(|p| p.page), Some(2));
}

#[test]
fn a_lost_book_is_found_again_by_choosing_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let raven = put(&dir.path().join("in"), "raven.epub", &raven_bytes());
    let mut h = reader(&[("library", "show")]);
    act(&mut h, read_in(dir.path().join("in")));
    let id = library(&h).book_at(&raven).unwrap();
    let elsewhere = moved(&raven, &dir.path().join("out/renamed.epub"));
    act(&mut h, Action::Rescan);
    assert!(library(&h).book(id).unwrap().missing());
    act(&mut h, Action::FindAgain(id));
    let path = elsewhere.clone();
    act(&mut h, Action::Relink { book: id, path });
    assert!(!library(&h).book(id).unwrap().missing(), "{}", state(&h));
    assert_eq!(library(&h).book_at(&elsewhere), Some(id));
    assert!(!h.state().library_showing(), "a found book opens");
}

#[test]
fn searching_a_folder_finds_a_lost_book_under_any_name() {
    let dir = tempfile::tempdir().unwrap();
    let raven = put(&dir.path().join("in"), "raven.epub", &raven_bytes());
    let mut h = reader(&[("library", "show")]);
    act(&mut h, read_in(dir.path().join("in")));
    let id = library(&h).book_at(&raven).unwrap();
    let deep = moved(&raven, &dir.path().join("attic/old/poems.epub"));
    act(&mut h, Action::Rescan);
    act(&mut h, Action::FindAgain(id));
    let folder = dir.path().join("attic");
    act(&mut h, Action::Seek { book: id, folder });
    assert_eq!(library(&h).book_at(&deep), Some(id), "{}", state(&h));
}

#[test]
fn a_namesake_with_other_content_is_offered_not_taken() {
    let dir = tempfile::tempdir().unwrap();
    let notes = put(&dir.path().join("in"), "notes.md", NOTES);
    let mut h = reader(&[("library", "show")]);
    act(&mut h, read_in(dir.path().join("in")));
    let id = library(&h).book_at(&notes).unwrap();
    let before = library(&h).book(id).unwrap().content;
    std::fs::remove_file(&notes).unwrap();
    let edited = put(
        &dir.path().join("backup"),
        "notes.md",
        b"# One\n\nText, revised.\n",
    );
    act(&mut h, Action::Rescan);
    act(&mut h, Action::FindAgain(id));
    let folder = dir.path().join("backup");
    act(&mut h, Action::Seek { book: id, folder });
    assert!(library(&h).book(id).unwrap().missing(), "asked first");
    h.get_by_label("Use this file").click();
    h.run();
    h.run();
    let book = library(&h).book(id).unwrap();
    assert!(!book.missing(), "{}", state(&h));
    assert_ne!(book.content, before);
    assert_eq!(library(&h).book_at(&edited), Some(id));
}

/// The note an import that found nothing new shows; closing lights its shelf.
fn nothing_new(h: &mut Harness<'static, MareaderApp>) -> ShelfId {
    let sheet = h.state().library_state().sheet().cloned();
    let Some(Sheet::Note {
        shelf,
        returned: false,
    }) = sheet
    else {
        panic!("no note: {sheet:?}\n{}", state(h));
    };
    assert!(shows(h, "Nothing new to import"));
    h.get_by_label("Show the shelf").click();
    h.run();
    assert!(h.state().library_state().sheet().is_none());
    assert_eq!(
        h.state().library_state().revealed(),
        Some(Pick::Shelf(shelf))
    );
    shelf
}

#[test]
fn importing_a_folder_again_brings_nothing_twice() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Shelf");
    let raven = put(&root, "raven.epub", &raven_bytes());
    put(&root, "notes.md", NOTES);
    let mut h = reader(&[("library", "show")]);
    act(&mut h, read_in(root.clone()));
    let id = library(&h).book_at(&raven).unwrap();
    act(&mut h, read_in(root));
    let shelf = nothing_new(&mut h);
    assert_eq!(shelf, library(&h).children(None)[0].id, "its own shelf");
    assert_eq!(library(&h).len(), 2);
    assert_eq!(library(&h).book_at(&raven), Some(id), "the same book");
    assert_eq!(library(&h).children(None).len(), 1, "one shelf");
}

#[test]
fn adding_a_folder_leaves_other_missing_books_missing() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "notes.md", NOTES);
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let lost = library(&h).books().filter(|b| b.missing()).count();
    assert_eq!(lost, 1);
    act(&mut h, read_in(dir.path()));
    assert_eq!(library(&h).len(), 5, "{}", state(&h));
    assert_eq!(library(&h).books().filter(|b| b.missing()).count(), 1);
}

#[test]
fn an_empty_library_offers_to_add_a_folder() {
    let mut h = reader(&[("library", "show")]);
    h.get_by_label("Add books").click();
    h.run();
    assert!(h.query_by_label("Choose a folder…").is_some());
}

/// Opens the plus card's menu and picks `entry`.
fn from_add_menu(h: &mut Harness<'static, MareaderApp>, entry: &str) {
    h.get_by_label("Add books").click();
    h.run();
    h.get_by_label(entry).click();
    h.run();
}

/// Reads a folder in place and watches it, every file counted.
fn read_in(root: impl Into<PathBuf>) -> Action {
    let opts = mareader_library::Options {
        watch: true,
        min_size: 0,
        ..mareader_library::Options::default()
    };
    let root = root.into();
    Action::ImportFolder { root, opts }
}

fn open_shelf(h: &Harness<'static, MareaderApp>) -> Option<ShelfId> {
    h.state().library_state().at()
}

fn shelve(h: &mut Harness<'static, MareaderApp>, action: ShelfAction) {
    act(h, Action::Shelf(action));
}

/// Carries books from one level to another, as a drop does.
fn file(books: &[BookId], from: Option<ShelfId>, to: Option<ShelfId>, keep: bool) -> ShelfAction {
    ShelfAction::Carry(Move {
        books: books.to_vec(),
        shelves: Vec::new(),
        links: Vec::new(),
        from,
        to,
        keep,
        before: None,
    })
}

/// A shelf made through the page's own action, at the top level.
fn new_shelf(h: &mut Harness<'static, MareaderApp>, name: &str) -> ShelfId {
    shelve(h, ShelfAction::New);
    let id = open_shelf(h).expect("a new shelf opens");
    act(h, Action::Rename(Named::Shelf(id), name.into()));
    shelve(h, ShelfAction::GoTo(None));
    id
}

/// The field a name is typed in; the bar's search is the other.
fn name_field<'a>(h: &'a Harness<'static, MareaderApp>) -> egui_kittest::Node<'a> {
    let search = Some(mareader_ui::library::search::LABEL.to_owned());
    let mut fields =
        (h.get_all_by_role(Role::TextInput)).filter(|n| n.accesskit_node().label() != search);
    fields.next().expect("a name field")
}

fn title_of(h: &Harness<'static, MareaderApp>, n: usize) -> String {
    let mut books: Vec<_> = library(h).books().collect();
    books.sort_by_key(|b| b.id);
    books[n].title.value.clone()
}

fn book_id(h: &Harness<'static, MareaderApp>, n: usize) -> BookId {
    let mut ids: Vec<BookId> = library(h).books().map(|b| b.id).collect();
    ids.sort();
    ids[n]
}

/// Opens the crumb menu of the shelf on the page and picks Rename….
fn rename_from_crumb(h: &mut Harness<'static, MareaderApp>, name: &str) {
    let options = format!("{name} shelf options");
    h.get_by_role_and_label(Role::Button, &options).click();
    h.run();
    h.get_by_label("Rename…").click();
    h.run();
}

#[test]
fn a_new_shelf_opens_and_is_named_in_the_breadcrumb() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    from_add_menu(&mut h, "New shelf");
    let id = open_shelf(&h).expect("the new shelf opens");
    assert_eq!(h.state().library_state().renaming(), None, "only opened");
    rename_from_crumb(&mut h, "New shelf");
    assert_eq!(h.state().library_state().renaming(), Some(id));
    // Its whole name is selected, so typing replaces it.
    let field = name_field(&h);
    assert!(field.is_focused());
    field.type_text("Poetry");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let shelf = library(&h).shelf(id).unwrap();
    assert_eq!(shelf.name.value, "Poetry");
    assert_eq!(h.state().library_state().renaming(), None);
    assert!(h.state().library_showing());
    h.get_by_role_and_label(Role::Button, "Home").click();
    h.run();
    assert_eq!(open_shelf(&h), None);
    h.get_by_label("Poetry").click();
    h.run();
    assert_eq!(open_shelf(&h), Some(id));
}

#[test]
fn the_breadcrumb_starts_at_home_and_names_its_way_back() {
    let mut h = reader(&[("books", "2"), ("library", "show")]);
    let trail = h.state().library_state().trail();
    assert_eq!(trail.len(), 1, "the top is Home alone");
    assert!(h.query_by_role_and_label(Role::Button, "Home").is_some());
    let outer = new_shelf(&mut h, "Outer");
    shelve(&mut h, ShelfAction::GoTo(Some(outer)));
    let inner = new_shelf(&mut h, "Inner");
    shelve(&mut h, ShelfAction::GoTo(Some(inner)));
    assert!(h.query_by_label("Go back to Outer").is_some());
    let options = h.get_by_label("Inner shelf options");
    options.click();
    h.run();
    assert!(shows(&h, "Rename…") && shows(&h, "Duplicate") && shows(&h, "Remove shelf"));
    assert!(!shows(&h, WATCHED_NOTE), "a hand-made shelf");
    h.get_by_label("Remove shelf").click();
    h.run();
    assert!(library(&h).shelf(inner).is_none());
    assert_eq!(open_shelf(&h), Some(outer));
    h.get_by_label("Outer shelf options");
}

#[test]
fn escape_while_naming_keeps_the_old_name_and_the_page() {
    let mut h = reader(&[("books", "2"), ("library", "show")]);
    from_add_menu(&mut h, "New shelf");
    let id = open_shelf(&h).unwrap();
    rename_from_crumb(&mut h, "New shelf");
    name_field(&h).type_text("Junk");
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(library(&h).shelf(id).unwrap().name.value, "New shelf");
    assert_eq!(h.state().library_state().renaming(), None);
    assert!(h.state().library_showing(), "Escape only ended the naming");
    // The crumb's menu names it again.
    rename_from_crumb(&mut h, "New shelf");
    assert_eq!(h.state().library_state().renaming(), Some(id));
}

#[test]
fn a_shelf_may_take_a_name_already_beside_it() {
    let mut h = reader(&[("books", "2"), ("library", "show")]);
    new_shelf(&mut h, "Poetry");
    let other = new_shelf(&mut h, "Other");
    act(&mut h, Action::Rename(Named::Shelf(other), "poetry".into()));
    assert_eq!(library(&h).shelf(other).unwrap().name.value, "poetry");
    assert_eq!(library(&h).children(None).len(), 2);
}

#[test]
fn dragging_a_book_onto_a_shelf_files_it_and_says_so() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let shelf = new_shelf(&mut h, "Poetry");
    let title = title_of(&h, 1);
    let book = h.get_by_label(&title).rect().center();
    let target = h.get_by_label("Poetry").rect().center();
    carry(&mut h, book, target);
    release(&mut h, target);
    assert_eq!(
        library(&h).shelves_of(book_id(&h, 1)).len(),
        1,
        "{}",
        toast(&h)
    );
    assert_eq!(toast(&h), format!("Added “{title}” to “Poetry”."));
    assert_eq!(open_shelf(&h), None, "a drop doesn't open the shelf");
    // Again: it's already there.
    carry(&mut h, book, target);
    release(&mut h, target);
    assert_eq!(toast(&h), format!("“{title}” is already on “Poetry”."));
    assert_eq!(library(&h).shelf_books(shelf).len(), 1);
}

#[test]
fn escape_drops_what_is_carried_and_keeps_the_page() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    new_shelf(&mut h, "Poetry");
    let title = title_of(&h, 2);
    let book = h.get_by_label(&title).rect().center();
    let target = h.get_by_label("Poetry").rect().center();
    carry(&mut h, book, target);
    h.key_press(egui::Key::Escape);
    h.run();
    release(&mut h, target);
    assert!(library(&h).shelved().is_empty(), "{}", toast(&h));
    assert!(
        h.state().library_showing(),
        "Escape ended the drag, not the page"
    );
}

#[test]
fn a_shelf_moves_books_between_shelves_or_adds_them_with_alt() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let a = new_shelf(&mut h, "A");
    let b = new_shelf(&mut h, "B");
    let book = book_id(&h, 0);
    shelve(&mut h, file(&[book], None, Some(a), false));
    shelve(&mut h, file(&[book], Some(a), Some(b), false));
    assert!(toast(&h).starts_with("Moved"), "{}", toast(&h));
    assert_eq!(
        library(&h).shelves_of(book).into_iter().collect::<Vec<_>>(),
        [b]
    );
    shelve(&mut h, file(&[book], Some(b), Some(a), true));
    assert_eq!(library(&h).shelves_of(book).len(), 2, "Alt keeps it on B");
    // Dropped on the top level's crumb, it leaves the open shelf.
    shelve(&mut h, file(&[book], Some(a), None, false));
    assert_eq!(
        library(&h).shelves_of(book).into_iter().collect::<Vec<_>>(),
        [b]
    );
    // A shelf can't go inside its own shelves.
    let shelf = |shelf, to| {
        ShelfAction::Carry(Move {
            books: Vec::new(),
            shelves: vec![shelf],
            links: Vec::new(),
            from: None,
            to,
            keep: false,
            before: None,
        })
    };
    let inner = new_shelf(&mut h, "Inner");
    shelve(&mut h, shelf(inner, Some(b)));
    assert_eq!(toast(&h), "Moved “Inner” to “B”.");
    shelve(&mut h, shelf(b, Some(inner)));
    shelve(&mut h, shelf(b, Some(b)));
    let parent = |id| library(&h).shelf(id).map(|s| s.parent.value);
    assert_eq!(parent(b), Some(None), "B stays at the top");
    assert_eq!(parent(inner), Some(Some(b)));
}

#[test]
fn home_can_leave_shelved_books_to_their_shelves() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let shelf = new_shelf(&mut h, "Poetry");
    let shelved = title_of(&h, 0);
    let first = book_id(&h, 0);
    shelve(&mut h, file(&[first], None, Some(shelf), false));
    assert!(
        h.query_by_label(&shelved).is_some(),
        "everything, by default"
    );
    h.get_by_label("View options").click();
    h.run();
    h.get_by_label("Unshelved").click();
    h.run();
    assert_eq!(h.state().settings().library.home, Home::Unshelved);
    assert!(h.query_by_label(&shelved).is_none());
    assert!(h.query_by_label(&title_of(&h, 1)).is_some());
}

#[test]
fn taking_a_shelf_apart_moves_its_shelves_up() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let outer = new_shelf(&mut h, "Outer");
    shelve(&mut h, ShelfAction::GoTo(Some(outer)));
    shelve(&mut h, ShelfAction::New);
    let inner = open_shelf(&h).unwrap();
    let first = book_id(&h, 0);
    shelve(&mut h, file(&[first], None, Some(outer), false));
    shelve(&mut h, ShelfAction::GoTo(Some(outer)));
    // Nothing is read in place, so it goes without asking.
    shelve(&mut h, ShelfAction::Apart(outer));
    assert!(library(&h).shelf(outer).is_none());
    assert_eq!(open_shelf(&h), None, "back up to where it was");
    let top: Vec<ShelfId> = library(&h).children(None).iter().map(|s| s.id).collect();
    assert_eq!(top, [inner]);
    assert_eq!(library(&h).len(), 4, "its books stay");
    assert_eq!(
        toast(&h),
        "Took “Outer” apart. Its books stay in the library."
    );
}

#[test]
fn a_long_trail_folds_its_middle_and_leads_back_up() {
    let mut h = reader(&[("books", "1"), ("library", "show")]);
    for n in 1..=5 {
        let id = new_shelf(&mut h, &format!("A shelf with a long name, level {n}"));
        shelve(&mut h, ShelfAction::GoTo(Some(id)));
    }
    h.run();
    let trail = h.state().library_state().trail();
    assert_eq!(trail.len(), 6, "Home and five levels");
    // Too wide for the bar: the oldest levels fold, never just one.
    assert!(h.query_by_label("Show the 1 levels above").is_none());
    let fold = h.get_by_label_contains("levels above");
    let open = format!("{} shelf options", trail[5].name);
    assert!(h.query_by_label(&open).is_some(), "the open level shows");
    fold.click();
    h.run();
    let second = trail[1].id;
    h.get_by_label(&format!("Go back to {}", trail[1].name))
        .click();
    h.run();
    assert_eq!(open_shelf(&h), second);
    h.get_by_role_and_label(Role::Button, "Home").click();
    h.run();
    assert_eq!(open_shelf(&h), None);
}

#[test]
fn a_removed_open_shelf_gives_way_to_its_parent() {
    let mut h = reader(&[("books", "2"), ("library", "show")]);
    let outer = new_shelf(&mut h, "Outer");
    shelve(&mut h, ShelfAction::GoTo(Some(outer)));
    shelve(&mut h, ShelfAction::New);
    let inner = open_shelf(&h).unwrap();
    shelve(&mut h, ShelfAction::Apart(inner));
    assert_eq!(open_shelf(&h), Some(outer));
    assert_eq!(h.state().library_state().renaming(), None);
}

fn picks(h: &Harness<'static, MareaderApp>) -> Vec<BookId> {
    h.state().library_state().picks().books()
}

fn book(h: &mut Harness<'static, MareaderApp>, action: BookAction) {
    act(h, Action::Book(action));
}

#[test]
fn ctrl_click_starts_a_selection_that_plain_clicks_then_extend() {
    let mut h = reader(&[("books", "6"), ("library", "show")]);
    let (one, two) = (title_of(&h, 1), title_of(&h, 2));
    h.get_by_label(&one)
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    assert_eq!(picks(&h), [book_id(&h, 1)]);
    assert!(h.query_by_label("1 selected").is_some(), "the bar shows");
    // While picking, a plain click picks too instead of opening.
    h.get_by_label(&two).click();
    h.run();
    assert_eq!(picks(&h).len(), 2);
    assert!(h.state().library_showing(), "nothing opened");
    h.get_by_label(&two).click();
    h.run();
    assert_eq!(picks(&h), [book_id(&h, 1)]);
    // Escape lets go of the selection before it leaves the page.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(picks(&h).is_empty());
    assert!(h.state().library_showing());
}

#[test]
fn shift_click_picks_the_run_between_and_ctrl_a_picks_all() {
    let mut h = reader(&[("books", "6"), ("library", "show"), ("layout", "list")]);
    // In the order shown: top to bottom.
    let mut titles: Vec<String> = library(&h).books().map(|b| b.title.value.clone()).collect();
    titles.sort_by(|a, b| top(&h, a).total_cmp(&top(&h, b)));
    h.get_by_label(&titles[1])
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    h.get_by_label(&titles[4])
        .click_modifiers(egui::Modifiers::SHIFT);
    h.run();
    assert_eq!(picks(&h).len(), 4, "{titles:?}");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    assert_eq!(picks(&h).len(), 6);
    h.get_by_label("Done").click();
    h.run();
    assert!(picks(&h).is_empty());
}

#[test]
fn picks_go_on_a_new_shelf_together_and_come_off_it() {
    let mut h = reader(&[("books", "5"), ("library", "show")]);
    let (a, b) = (book_id(&h, 1), book_id(&h, 2));
    let both = vec![Pick::Book(a), Pick::Book(b)];
    act(&mut h, Action::Pick(PickAction::Only(both)));
    h.get_by_label("Add to shelf").click();
    h.run();
    h.get_by_label("New shelf").click();
    h.run();
    assert_eq!(open_shelf(&h), None, "the page stays where it was");
    let shelf = library(&h).children(None)[0].id;
    assert_eq!(library(&h).shelf_books(shelf).len(), 2);
    assert!(picks(&h).is_empty());
    shelve(&mut h, ShelfAction::GoTo(Some(shelf)));
    act(&mut h, Action::Pick(PickAction::Only(vec![Pick::Book(a)])));
    shelve(&mut h, file(&[a], Some(shelf), None, false));
    assert_eq!(library(&h).shelf_books(shelf).len(), 1);
    assert!(toast(&h).starts_with("Took “"), "{}", toast(&h));
}

#[test]
fn the_bar_adds_picks_to_a_shelf_they_may_go_on() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let poetry = new_shelf(&mut h, "Poetry");
    let prose = new_shelf(&mut h, "Prose");
    let a = book_id(&h, 1);
    let picked = vec![Pick::Book(a), Pick::Shelf(poetry)];
    act(&mut h, Action::Pick(PickAction::Only(picked)));
    assert!(shows(&h, "2 selected"));
    h.get_by_label("Add to shelf").click();
    h.run();
    // A shelf can't go inside itself: Poetry has its card only.
    assert_eq!(h.query_all_by_label("Poetry").count(), 1);
    let rows = h.get_all_by_label("Prose");
    let row = rows.max_by(|x, y| x.rect().top().total_cmp(&y.rect().top()));
    row.expect("offered").click();
    h.run();
    assert!(library(&h).shelf_books(prose).iter().any(|b| b.id == a));
    let lib = library(&h);
    assert_eq!(
        lib.shelf(poetry).and_then(|s| lib.parent_of(s)),
        Some(prose)
    );
    assert!(
        !h.state().library_state().picks().active(),
        "done selecting"
    );
}

#[test]
fn a_dragged_selection_moves_onto_a_shelf_together() {
    let mut h = reader(&[("books", "5"), ("library", "show")]);
    let shelf = new_shelf(&mut h, "Poetry");
    let (one, two) = (title_of(&h, 1), title_of(&h, 2));
    h.get_by_label(&one)
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    h.get_by_label(&two)
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    let from = h.get_by_label(&one).rect().center();
    let target = h.get_by_label("Poetry").rect().center();
    carry(&mut h, from, target);
    release(&mut h, target);
    assert_eq!(library(&h).shelf_books(shelf).len(), 2, "{}", toast(&h));
    assert_eq!(toast(&h), "Added 2 books to “Poetry”.");
    assert!(picks(&h).is_empty(), "a drop lets go");
}

#[test]
fn a_card_menu_offers_what_a_book_can_do() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let title = title_of(&h, 1);
    h.get_by_label(&title).click_secondary();
    h.run();
    let entries = [
        "Open",
        "Select",
        "Rename…",
        "Duplicate",
        "Read separately",
        "Remove from library",
    ];
    for entry in entries {
        assert!(h.query_by_label(entry).is_some(), "{entry}");
    }
    h.get_by_label("Rename…").click();
    h.run();
    // The old name starts focused and selected: typing replaces it.
    assert!(name_field(&h).is_focused());
    name_field(&h).type_text("Moby Dick");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let renamed = library(&h).book(book_id(&h, 1)).unwrap();
    assert_eq!(renamed.title.value, "Moby Dick");
    assert!(h.state().library_state().sheet().is_none());
}

#[test]
fn removing_a_folder_book_hides_it_until_restored_from_its_menu() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Shelf");
    let raven = put(&root, "raven.epub", &raven_bytes());
    put(&root, "notes.md", NOTES);
    let mut h = reader(&[("library", "show")]);
    act(&mut h, read_in(root.clone()));
    let id = library(&h).book_at(&raven).unwrap();
    let (books, shelves, links) = (vec![id], Vec::new(), Vec::new());
    book(
        &mut h,
        BookAction::AskRemove {
            books,
            shelves,
            links,
        },
    );
    let says = "The file stays on disk. Its folder won't add it back.";
    assert!(
        h.query_by_label(says).is_some(),
        "the alert says what stays"
    );
    let name = library(&h).book(id).unwrap().title.value.clone();
    let asks = format!("Remove “{name}”?");
    assert!(h.query_by_label(&asks).is_some(), "one book goes by name");
    h.get_by_label("Remove").click();
    h.run();
    assert!(library(&h).book(id).is_none());
    assert!(raven.is_file(), "the file is never touched");
    assert!(
        toast(&h).ends_with("The folder's Add menu can give them back."),
        "{}",
        toast(&h)
    );
    act(&mut h, Action::Rescan);
    assert!(
        library(&h).book_at(&raven).is_none(),
        "a rescan leaves it out"
    );
    // The Undo toast sits over the menu's last rows until it goes.
    h.run_steps(((mareader_ui::study::UNDO_FOR + 1.0) * 60.0) as usize);
    assert_eq!(toast(&h), "");
    // The folder's own shelf lists it under Restore.
    let shelf = library(&h).children(None)[0].id;
    shelve(&mut h, ShelfAction::GoTo(Some(shelf)));
    h.get_by_label("Add books").click();
    h.run();
    h.get_by_label("The Raven").click();
    h.run();
    h.run();
    let back = library(&h).book_at(&raven);
    let back = back.unwrap_or_else(|| panic!("not restored: {}", state(&h)));
    assert!(!library(&h).book(back).unwrap().missing());
    assert_eq!(library(&h).shelf_books(shelf).len(), 2, "back on its shelf");
    assert_eq!(toast(&h), "Restored “The Raven”.");
}

#[test]
fn erasing_a_position_restarts_an_open_book_before_it_returns() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Shelf");
    let path = put(&root, "raven.epub", &raven_bytes());
    let mut h = reader(&[("library", "show")]);
    act(&mut h, read_in(root.clone()));
    let id = library(&h).book_at(&path).unwrap();
    let content = library(&h).book(id).unwrap().content;
    let folder = library(&h).folder_at(&root, true).unwrap();

    h.state_mut().queue(Command::OpenBook(id));
    h.run();
    h.run_steps(3);
    go_to(&mut h, 4);
    h.run_steps(3);
    assert_eq!(library(&h).position(id).map(|p| p.page), Some(4));
    h.state_mut().queue(Command::ShowLibrary);
    h.run();

    book(
        &mut h,
        BookAction::AskRemove {
            books: vec![id],
            shelves: Vec::new(),
            links: Vec::new(),
        },
    );
    let erase = "Erase highlights, AI pages and reading position";
    assert!(h.query_by_role_and_label(Role::CheckBox, erase).is_some());
    h.get_by_role_and_label(Role::CheckBox, erase).click();
    h.run();
    h.get_by_label("Erase & remove").click();
    h.run_steps(5);

    assert!(library(&h).book(id).is_none());
    assert_eq!(
        library(&h).position(id).map(|p| (p.page, p.pages)),
        Some((0, 0))
    );
    let pane = h.state().active_pane();
    assert_eq!(h.state().documents().current(pane), Some(0));

    act(&mut h, Action::Restore { folder, content });
    assert_eq!(library(&h).book_at(&path), Some(id));
    assert_eq!(page(&h), 0, "the old open view cannot restore page 4");
    assert_ne!(library(&h).position(id).map(|p| p.page), Some(4));
}

#[test]
fn read_separately_keeps_a_second_place_in_the_same_file() {
    let path = raven();
    let mut h = reader(&[("epub", path.to_str().unwrap())]);
    go_to(&mut h, 2);
    h.run_steps(3);
    let id = library(&h).book_at(&path).unwrap();
    book(&mut h, BookAction::Twin(id));
    let twin = library(&h).books().find(|b| b.twin_of.value == Some(id));
    let twin = twin
        .map(|b| b.id)
        .unwrap_or_else(|| panic!("{}", state(&h)));
    assert_eq!(library(&h).book(twin).unwrap().title.value, "The Raven (2)");
    h.state_mut().queue(Command::OpenBook(twin));
    h.run();
    h.run_steps(3);
    assert_eq!(page(&h), 0, "a twin starts at its own place");
    let last = h.state().last_document().map(|l| l.book);
    assert_eq!(last, Some(Some(twin)), "a relaunch reopens the twin");
    go_to(&mut h, 1);
    h.run_steps(3);
    assert_eq!(library(&h).position(twin).map(|p| p.page), Some(1));
    assert_eq!(library(&h).position(id).map(|p| p.page), Some(2));
    h.state_mut().queue(Command::OpenBook(id));
    h.run();
    h.run_steps(3);
    assert_eq!(page(&h), 2, "the original kept its place");
}

/// The Raven, read to page 2, and a twin of it.
fn raven_and_twin(h: &mut Harness<'static, MareaderApp>, path: &Path) -> (BookId, BookId) {
    go_to(h, 2);
    h.run_steps(3);
    let id = library(h).book_at(path).unwrap();
    book(h, BookAction::Twin(id));
    let twin = library(h).books().find(|b| b.twin_of.value == Some(id));
    (
        id,
        twin.map(|b| b.id).unwrap_or_else(|| panic!("{}", state(h))),
    )
}

#[test]
fn a_failed_open_leaves_the_next_open_as_the_files_own_book() {
    let dir = tempfile::tempdir().unwrap();
    let path = put(dir.path(), "raven.epub", &raven_bytes());
    let mut h = reader(&[("epub", path.to_str().unwrap())]);
    let (id, twin) = raven_and_twin(&mut h, &path);
    // Shown nowhere, then unreadable: opening the twin fails.
    open(&mut h, "pdf", "sample:3");
    std::fs::write(&path, b"not a book").unwrap();
    h.state_mut().queue(Command::OpenBook(twin));
    h.run();
    h.run_steps(3);
    assert!(toast(&h).starts_with("Couldn't open"), "{}", state(&h));
    std::fs::write(&path, raven_bytes()).unwrap();
    open(&mut h, "epub", path.to_str().unwrap());
    h.run_steps(3);
    let pane = h.state().active_pane();
    assert_eq!(
        h.state().documents().book(pane),
        None,
        "the file's own book"
    );
    assert_eq!(page(&h), 2, "at the original's place: {}", state(&h));
    go_to(&mut h, 3);
    h.run_steps(3);
    assert_eq!(library(&h).position(id).map(|p| p.page), Some(3));
    assert_ne!(library(&h).position(twin).map(|p| p.page), Some(3));
}

#[test]
fn a_twin_opened_beside_its_original_leaves_that_pane_alone() {
    let path = raven();
    let mut h = reader(&[("epub", path.to_str().unwrap())]);
    let first = h.state().active_pane();
    let (id, twin) = raven_and_twin(&mut h, &path);
    let split = Command::Split(mareader_workspace::SplitAxis::Horizontal);
    h.state_mut().queue(split);
    h.run();
    h.state_mut().queue(Command::OpenBook(twin));
    h.run();
    h.run_steps(3);
    let beside = h.state().active_pane();
    assert_ne!(beside, first);
    assert_eq!(h.state().documents().book(beside), Some(twin));
    assert_eq!(
        h.state().documents().book(first),
        None,
        "still the original"
    );
    h.state_mut().queue(Command::FocusPane(first));
    h.run();
    go_to(&mut h, 3);
    h.run_steps(3);
    assert_eq!(library(&h).position(id).map(|p| p.page), Some(3));
    assert_ne!(library(&h).position(twin).map(|p| p.page), Some(3));
}

#[test]
fn imports_are_copies_the_library_keeps_and_deletes_with_the_book() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    let outside = put(dir.path(), "Raven.epub", &raven_bytes());
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("library", "show").ok();
    app.attach_library(root.clone());
    let mut h = harness(app);
    let shelf = new_shelf(&mut h, "Poems");
    let to = Some(shelf);
    book(
        &mut h,
        BookAction::Import {
            paths: vec![outside.clone()],
            to,
        },
    );
    assert_eq!(toast(&h), "Imported “The Raven” to “Poems”.");
    let b = library(&h).books().next().unwrap().clone();
    let copy = b.path().unwrap().to_path_buf();
    assert_eq!(b.locations[0].origin, Origin::Store);
    assert!(copy.starts_with(root.join("files").canonicalize().unwrap()));
    assert_eq!(library(&h).shelf_books(shelf).len(), 1);
    // The same content again asks first; nothing is copied meanwhile.
    let notes = put(dir.path(), "notes.docx", b"not a book");
    book(
        &mut h,
        BookAction::Import {
            paths: vec![outside.clone(), notes],
            to: None,
        },
    );
    assert!(h.query_by_label("Already in your library").is_some());
    assert_eq!(library(&h).len(), 1);
    h.get_by_label("Show the imported one").click();
    h.run();
    assert!(h.state().library_state().questions().next().is_none());
    assert_eq!(library(&h).len(), 1, "going to it adds nothing");
    assert_eq!(open_shelf(&h), Some(shelf), "it shows where it is");
    let (books, shelves, links) = (vec![b.id], Vec::new(), Vec::new());
    book(
        &mut h,
        BookAction::AskRemove {
            books,
            shelves,
            links,
        },
    );
    let says = "The library's copy is deleted.";
    assert!(
        h.query_by_label(says).is_some(),
        "the alert says the copy goes"
    );
    h.get_by_label("Remove").click();
    h.run();
    h.run();
    assert!(!copy.exists(), "the library's copy goes");
    assert!(outside.is_file(), "the file imported from stays");
}

/// A library kept in `dir/library`, its page showing.
fn kept_in(dir: &Path) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("library", "show").ok();
    app.attach_library(dir.join("library"));
    harness(app)
}

/// Steps the import sheet's size floor down to nothing.
fn every_size(h: &mut Harness<'static, MareaderApp>) {
    for _ in 0..3 {
        h.get_by_label("Smaller files count too").click();
        h.run_steps(30);
    }
}

#[test]
fn a_folder_copied_in_takes_only_what_counts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Shelf");
    put(&root, "raven.epub", &raven_bytes());
    put(&root, "deep/notes.md", NOTES);
    let empty = dir.path().join("Empty");
    std::fs::create_dir_all(&empty).unwrap();
    let mut h = kept_in(dir.path());
    act(&mut h, Action::AskFolder(root.clone()));
    assert!(h.query_by_label("Import a folder").is_some());
    // Unticked, the books are copied in.
    h.get_by_label("Read books in place").click();
    h.run();
    // Unticked, Markdown is excluded.
    h.get_by_label("MD").click();
    h.run();
    every_size(&mut h);
    h.get_by_label("Import").click();
    h.run();
    h.run();
    h.run();
    assert!(h.state().library_state().sheet().is_none());
    assert_eq!(library(&h).len(), 1, "{}", state(&h));
    let b = library(&h).books().next().unwrap();
    assert_eq!(b.locations[0].origin, Origin::Store);
    let folder = library(&h).folders().next().unwrap();
    assert!(folder.mode().copies(), "nothing is read in place");
    assert_eq!(toast(&h), "Added 1 book from “Shelf”.");
    let shelf = library(&h).children(None)[0].id;
    assert_eq!(library(&h).shelf(shelf).unwrap().name.value, "Shelf");
    assert_eq!(
        library(&h).shelf_books(shelf).len(),
        1,
        "a shelf per folder"
    );
    // The next sheet starts from these answers.
    act(&mut h, Action::AskFolder(empty));
    h.get_by_label("Import").click();
    h.run();
    h.run();
    h.run();
    assert_eq!(toast(&h), "No books found in “Empty”.");
}

#[test]
fn a_folder_read_in_place_keeps_the_sheets_choices() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Shelf");
    put(&root, "raven.epub", &raven_bytes());
    put(&root, "notes.md", NOTES);
    let mut h = kept_in(dir.path());
    act(&mut h, Action::AskFolder(root.clone()));
    // Read in place, unwatched, is where a new library starts.
    h.get_by_label("EPUB").click();
    h.run();
    every_size(&mut h);
    h.get_by_label("Import").click();
    h.run();
    h.run();
    let folder = library(&h).folders().next().unwrap().clone();
    assert!(!folder.opts.watch, "read in place, not watched");
    assert!(!folder.opts.formats.contains(&Format::Epub));
    assert_eq!(library(&h).len(), 1, "{}", state(&h));
    assert_eq!(title_of(&h, 0), "Field Notes");
    let shelf = library(&h).children(None)[0];
    assert_eq!(shelf.name.value, "Shelf", "a shelf for the folder");
    assert_eq!(library(&h).shelf_books(shelf.id).len(), 1);
    let last = h.state().library_state().last_options();
    assert!(!last.formats.contains(&Format::Epub), "kept for next time");
}

#[test]
fn covers_scrolled_past_never_pile_up_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Many");
    for n in 0..120 {
        let pdf = mareader_pdf::sample::pdf(1, &format!("Book {n}"));
        put(&root, &format!("b{n:03}.pdf"), &pdf);
    }
    let mut h = kept_in(dir.path());
    act(&mut h, read_in(&root));
    assert_eq!(library(&h).len(), 120, "{}", state(&h));
    // A fast scroll, a frame a step, past far more covers than fit.
    h.hover_at(egui::pos2(450.0, 400.0));
    let mut most = 0;
    for _ in 0..80 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -300.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
        let waiting = h.state().library_state().covers().waiting();
        most = most.max(waiting);
    }
    // Read covers wait only while their books are near the view.
    assert!(most <= 8, "{most} covers waited for a texture");
    assert!(h.state().covers_shown() > 0, "covers were drawn");
    assert!(h.state().library_state().covers().bytes() <= 64 << 20);
}

fn note(title: &str) -> Vec<u8> {
    format!("---\ntitle: {title}\n---\n\n# {title}\n\nText.\n").into_bytes()
}

/// The crumb menu's word on a shelf a watched folder cut.
const WATCHED_NOTE: &str = "Cut from a watched folder: removing takes it off the list, and it \
                            returns if the folder places a book here again.";

fn shows(h: &Harness<'static, MareaderApp>, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

#[test]
fn a_folder_is_shelved_through_its_own_subfolders() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Books");
    put(&root, "top.md", &note("Top Note"));
    put(&root, "Poe/raven.md", &note("Raven Note"));
    put(&root, "Poe/Early/early.md", &note("Early Note"));
    put(&root, "Woolf/waves.md", &note("Waves Note"));
    let mut h = kept_in(dir.path());
    act(&mut h, read_in(&root));
    assert_eq!(library(&h).len(), 4, "{}", state(&h));
    // The run lights the folder's own shelf, at the top.
    let books = library(&h).children(None)[0].id;
    assert_eq!(library(&h).shelf(books).unwrap().name.value, "Books");
    shelve(&mut h, ShelfAction::GoTo(Some(books)));
    assert!(shows(&h, "Top Note"));
    assert!(
        shows(&h, "Poe") && shows(&h, "Woolf"),
        "a shelf per subfolder"
    );
    assert!(!shows(&h, "Raven Note"), "below, so not here");
    let poe = library(&h).children(Some(books));
    let poe = poe.iter().find(|s| s.name.value == "Poe").unwrap().id;
    shelve(&mut h, ShelfAction::GoTo(Some(poe)));
    assert!(shows(&h, "Raven Note"));
    assert!(shows(&h, "Early"));
    assert!(!shows(&h, "Top Note"));
    // A search at the top reaches every shelf below.
    shelve(&mut h, ShelfAction::GoTo(None));
    act(&mut h, Action::Search(SearchAction::Text("early".into())));
    assert!(shows(&h, "Early Note"));
}

#[test]
fn the_plus_card_ends_the_page_but_not_a_search() {
    let mut h = reader(&[("books", "3"), ("library", "show")]);
    h.get_by_label("Add books").click();
    h.run();
    for entry in ["Choose files…", "Choose a folder…", "New shelf"] {
        assert!(h.query_by_label(entry).is_some(), "{entry}");
    }
    // Reading in place is the folder sheet's to offer.
    assert!(h.query_by_label("Add files in place…").is_none());
    h.key_press(egui::Key::Escape);
    h.run();
    let text = mareader_ui::library::SearchAction::Text("b".into());
    act(&mut h, Action::Search(text));
    assert!(h.query_by_label("Add books").is_none());
}

#[test]
fn a_book_read_in_place_asks_before_it_leaves_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Books");
    put(&root, "top.md", &note("Top Note"));
    let raven = put(&root, "Poe/raven.md", &note("Raven Note"));
    let mut h = kept_in(dir.path());
    act(&mut h, read_in(&root));
    let books = library(&h).children(None)[0].id;
    let poe = library(&h).children(Some(books))[0].id;
    let id = library(&h).book_at(&raven).unwrap();
    // Off the folder's shelves, it can only go as a copy: asked first.
    shelve(&mut h, file(&[id], Some(poe), None, false));
    assert!(shows(&h, "Copying books read in place"));
    assert!(shows(&h, "1 book from “Books”"), "{}", state(&h));
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().library_state().sheet().is_none());
    assert!(library(&h).shelves_of(id).contains(&poe), "left as it was");
    shelve(&mut h, file(&[id], Some(poe), None, false));
    h.get_by_label("Copy and move").click();
    h.run();
    h.run();
    let book = library(&h).book(id).unwrap();
    assert_eq!(book.locations[0].origin, Origin::Store, "{}", toast(&h));
    assert!(library(&h).shelves_of(id).is_empty(), "{}", toast(&h));
    assert!(raven.is_file(), "the folder on disk is untouched");
    // A rung taken apart sends its books up; the copy is asked too.
    let top = library(&h).book_at(&root.join("top.md").canonicalize().unwrap());
    shelve(&mut h, ShelfAction::Apart(books));
    assert!(shows(&h, "Take shelf apart"));
    assert!(
        library(&h).shelf(books).is_some(),
        "nothing before the answer"
    );
    assert!(top.is_some());
}

#[test]
fn a_move_onto_a_shelf_holding_the_name_asks_first() {
    let mut h = reader(&[("books", "4"), ("library", "show")]);
    let (a, b) = (new_shelf(&mut h, "A"), new_shelf(&mut h, "B"));
    let (kept, moving) = (book_id(&h, 0), book_id(&h, 1));
    let name = title_of(&h, 0);
    act(&mut h, Action::Rename(Named::Book(moving), name.clone()));
    shelve(&mut h, file(&[kept], None, Some(a), false));
    shelve(&mut h, file(&[moving], None, Some(b), false));
    shelve(&mut h, file(&[moving], Some(b), Some(a), false));
    assert!(h.state().library_state().questions().next().is_some());
    assert_eq!(library(&h).shelf_books(a).len(), 1, "nothing lands yet");
    h.get_by_label("As new").click();
    h.run();
    assert!(h.state().library_state().questions().next().is_none());
    assert_eq!(library(&h).shelf_books(a).len(), 2);
    assert!(library(&h).shelf_books(b).is_empty(), "moved, not added");
    let fresh = &library(&h).book(moving).unwrap().title.value;
    assert_ne!(fresh, &name, "kept apart by a new name");
}

/// A link made straight on the model, named, where the page is.
fn make_link(h: &mut Harness<'static, MareaderApp>, target: LinkTo, name: &str) -> LinkId {
    let state = h.state_mut().library_state_mut();
    let at = state.at();
    let model = state.model_mut();
    let id = model.link_to(target, at, 1).unwrap();
    assert!(model.rename_link(id, name, 2));
    state.settle();
    h.run();
    id
}

#[test]
fn a_link_lights_what_it_points_at_and_goes_without_it() {
    let mut h = reader(&[("books", "2"), ("library", "show")]);
    let dune = book_id(&h, 0);
    let kept = new_shelf(&mut h, "Kept");
    shelve(&mut h, file(&[dune], None, Some(kept), false));
    shelve(&mut h, ShelfAction::GoTo(None));
    let link = make_link(&mut h, LinkTo::Book(dune), "Pointer");
    h.get_by_label("Pointer").click();
    h.run();
    let lit = h.state().library_state().revealed();
    assert_eq!(
        lit,
        Some(Pick::Book(dune)),
        "it lights the book, never opens it"
    );
    assert_eq!(open_shelf(&h), Some(kept), "where the book is");
    shelve(&mut h, ShelfAction::GoTo(None));
    let links = vec![link];
    shelve(
        &mut h,
        ShelfAction::Carry(Move {
            links: links.clone(),
            to: Some(kept),
            ..Move::default()
        }),
    );
    assert_eq!(toast(&h), "Moved “Pointer” to “Kept”.");
    assert_eq!(library(&h).link(link).unwrap().level.value, Some(kept));
    book(
        &mut h,
        BookAction::AskRemove {
            books: Vec::new(),
            shelves: Vec::new(),
            links,
        },
    );
    let says = "The book it points to stays.";
    assert!(
        h.query_by_label(says).is_some(),
        "the alert says what stays"
    );
    assert!(h.query_by_label("Remove the link “Pointer”?").is_some());
    h.get_by_label("Remove").click();
    h.run();
    assert!(library(&h).link(link).is_none());
    assert!(library(&h).book(dune).is_some(), "the book stays");
    assert_eq!(toast(&h), "Removed the link “Pointer”.");
}

#[test]
fn removing_a_shelf_asks_in_a_short_alert_and_keeps_its_books() {
    let mut h = reader(&[("books", "2"), ("library", "show")]);
    let dune = book_id(&h, 0);
    let kept = new_shelf(&mut h, "Kept");
    shelve(&mut h, file(&[dune], None, Some(kept), false));
    shelve(&mut h, ShelfAction::GoTo(None));
    let shelves = vec![kept];
    book(
        &mut h,
        BookAction::AskRemove {
            books: Vec::new(),
            shelves,
            links: Vec::new(),
        },
    );
    assert!(h.query_by_label("Remove the shelf “Kept”?").is_some());
    let off = "Off, its books stay in the library.";
    assert!(
        h.query_by_label(off).is_some(),
        "the switch says what stays"
    );
    let inside = "Remove everything inside";
    h.get_by_role_and_label(Role::CheckBox, inside).click();
    h.run();
    let on = "1 book goes too.";
    assert!(h.query_by_label(on).is_some(), "on, it says what goes");
    h.get_by_role_and_label(Role::CheckBox, inside).click();
    h.run();
    h.get_by_label("Remove").click();
    h.run();
    assert!(library(&h).shelf(kept).is_none());
    assert!(library(&h).book(dune).is_some(), "its books stay");
    assert_eq!(toast(&h), "Removed “Kept”.");
}

#[test]
fn duplicating_a_book_makes_the_librarys_own_copy_under_the_next_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Shelf");
    let raven = put(&root, "raven.epub", &raven_bytes());
    let mut h = kept_in(dir.path());
    act(&mut h, read_in(root.clone()));
    let id = library(&h).book_at(&raven).unwrap();
    let title = library(&h).book(id).unwrap().title.value.clone();
    book(&mut h, BookAction::Duplicate(vec![Pick::Book(id)]));
    h.run();
    assert_eq!(toast(&h), format!("Duplicated as “{title}_1”."));
    let copy = library(&h).books().find(|b| b.id != id).unwrap();
    let home = copy.home().unwrap();
    assert_eq!(home.origin, Origin::Store, "the library's own");
    assert!(raven.is_file() && home.path != raven);
}

#[test]
fn a_folders_shelf_menu_watches_its_own_subfolder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Shelf");
    put(&root, "raven.epub", &raven_bytes());
    put(&root, "sub/notes.md", NOTES);
    let mut h = reader(&[("library", "show")]);
    act(&mut h, read_in(root.clone()));
    let named = |h: &Harness<'static, MareaderApp>, name: &str| {
        let mut all = library(h).shelves();
        all.find(|s| s.name.value == name).map(|s| s.id).unwrap()
    };
    let (top, sub) = (named(&h, "Shelf"), named(&h, "sub"));
    shelve(&mut h, ShelfAction::GoTo(Some(top)));
    let on = library(&h).shelf_tracked(sub);
    h.get_by_label("sub").click_secondary();
    h.run();
    let label = match on {
        true => "Stop watching this subfolder",
        false => "Watch this subfolder for new books",
    };
    assert!(
        h.query_by_label("Only “sub” and the folders inside it")
            .is_some()
    );
    assert!(h.query_by_label("Duplicate").is_some());
    h.get_by_label(label).click();
    h.run();
    h.run();
    let said = match on {
        true => "“sub” in Shelf is no longer watched for new books.",
        false => "Watching “sub” in Shelf for new books.",
    };
    assert_eq!(toast(&h), said);
    assert_eq!(library(&h).shelf_tracked(sub), !on);
    shelve(&mut h, ShelfAction::Watch(sub, true));
    shelve(&mut h, ShelfAction::GoTo(Some(sub)));
    h.get_by_label("sub shelf options").click();
    h.run();
    assert!(shows(&h, WATCHED_NOTE), "a watched folder's shelf says so");
}

#[test]
fn a_shelf_row_in_the_list_unfolds_in_place_and_its_button_opens_it() {
    let options = [
        ("books", "8"),
        ("shelves", "demo"),
        ("home", "unshelved"),
        ("layout", "list"),
        ("library", "show"),
    ];
    let mut h = reader(&options);
    let named = |h: &Harness<'static, MareaderApp>, name: &str| {
        let mut all = library(h).shelves();
        all.find(|s| s.name.value == name).map(|s| s.id).unwrap()
    };
    let (fiction, classics) = (named(&h, "Fiction"), named(&h, "Classics"));
    let model = library(&h);
    let only = (model.shelf_books(fiction).into_iter())
        .find(|b| !model.shelves_of(b.id).contains(&classics))
        .map(|b| b.title.value.clone())
        .unwrap();
    assert!(h.query_by_label("Classics").is_none(), "folded");
    h.get_by_label("Fiction").click();
    h.run();
    assert!(h.query_by_label("Classics").is_some(), "its shelves show");
    assert!(h.query_by_label(&only).is_some(), "and its books");
    assert_eq!(open_shelf(&h), None, "the page stays where it is");
    h.get_by_label("Fiction").click();
    h.run();
    assert!(h.query_by_label("Classics").is_none(), "it folds again");
    h.get_by_label("Open the Fiction shelf").click();
    h.run();
    assert_eq!(open_shelf(&h), Some(fiction));
}

#[test]
fn empty_space_context_menu_stays_compact() {
    let mut h = reader(&[("books", "3"), ("library", "show")]);
    let pos = egui::pos2(450.0, 620.0);
    h.hover_at(pos);
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(3);
    let row = h.get_by_label("New shelf").rect();
    assert!(
        row.width() <= 240.0,
        "menu row stretched across the page: {row:?}"
    );
}
