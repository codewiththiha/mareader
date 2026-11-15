use std::path::Path;

use crate::disk::{self, Folder};
use crate::test_support::{opened, page};
use crate::{BookId, ContentId, DeviceId, Format, Library, Opened};

const DEVICE: DeviceId = DeviceId(7);

#[test]
fn a_file_opened_twice_is_one_book_last_opened_first() {
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let b = lib.record_open(opened("/b.pdf", 2, Format::Pdf, None), 2_000);
    assert_eq!(
        lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 3_000),
        a
    );
    assert_eq!(lib.len(), 2);
    let order: Vec<BookId> = lib.recent().iter().map(|b| b.id).collect();
    assert_eq!(order, [a, b]);
    // The title is the document's at first sight; later opens keep it.
    let mut renamed = opened("/a.pdf", 1, Format::Pdf, None);
    renamed.title = "Other".into();
    lib.record_open(renamed, 4_000);
    assert_eq!(lib.book(a).map(|b| b.title.value.as_str()), Some("Book 1"));
}

#[test]
fn a_moved_file_is_the_same_book_and_keeps_its_place() {
    let mut lib = Library::new(DEVICE);
    let id = lib.record_open(opened("/old/a.pdf", 1, Format::Pdf, None), 1_000);
    lib.record_position(id, page(41, 100), 1_100);
    assert_eq!(
        lib.record_open(opened("/new/a.pdf", 1, Format::Pdf, None), 2_000),
        id
    );
    let book = lib.book(id).unwrap();
    assert_eq!(book.path(), Some(Path::new("/new/a.pdf")));
    assert_eq!(lib.book_at(Path::new("/new/a.pdf")), Some(id));
    assert_eq!(lib.position(id), Some(page(41, 100)));
}

#[test]
fn a_file_edited_in_place_is_the_same_book_with_new_content() {
    let mut lib = Library::new(DEVICE);
    let id = lib.record_open(opened("/notes.md", 1, Format::Pdf, None), 1_000);
    lib.record_position(id, page(3, 100), 1_100);
    assert_eq!(
        lib.record_open(opened("/notes.md", 2, Format::Pdf, None), 2_000),
        id
    );
    assert_eq!(lib.book(id).unwrap().content, ContentId(2));
    assert_eq!(lib.position(id), Some(page(3, 100)));
    assert_eq!(lib.len(), 1);
}

#[test]
fn a_path_holds_one_book() {
    // b.pdf is overwritten with a copy of a.pdf: it now reads book A.
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let b = lib.record_open(opened("/b.pdf", 2, Format::Pdf, None), 2_000);
    lib.record_open(opened("/b.pdf", 1, Format::Pdf, None), 3_000);
    assert_eq!(lib.book_at(Path::new("/b.pdf")), Some(a));
    assert!(lib.book(b).unwrap().missing());
    assert_eq!(lib.book(a).unwrap().locations.len(), 2);
}

#[test]
fn only_changes_are_written_and_positions_apart_from_books() {
    let mut lib = Library::new(DEVICE);
    let id = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let dirty = lib.take_dirty();
    assert_eq!(dirty.books.iter().collect::<Vec<_>>(), [&id]);
    assert!(dirty.states.is_empty());
    assert!(lib.record_position(id, page(5, 100), 1_100));
    assert!(!lib.record_position(id, page(5, 100), 1_200));
    let dirty = lib.take_dirty();
    assert!(dirty.store_is_clean(), "a page turn leaves the store alone");
    assert_eq!(dirty.states.into_iter().collect::<Vec<_>>(), [id]);
    assert!(!lib.is_dirty());
    // An unknown book has no state to write.
    assert!(!lib.record_position(BookId(99), page(1, 100), 1_300));
}

pub(crate) fn save(folder: &Folder, lib: &mut Library) {
    let mut store = folder.load(lib.device()).store;
    let dirty = lib.take_dirty();
    store.as_mut().unwrap().apply(&lib.changes(&dirty)).unwrap();
}

#[test]
fn the_library_comes_back_from_its_files_with_missing_books_marked() {
    let dir = tempfile::tempdir().unwrap();
    let book_file = dir.path().join("a.pdf");
    std::fs::write(&book_file, b"%PDF").unwrap();
    let folder = Folder::new(dir.path().join("library"));
    let mut lib = folder.load(DEVICE).library;
    let here = Opened {
        path: book_file.clone(),
        ..opened("", 1, Format::Pdf, None)
    };
    let a = lib.record_open(here, 1_000);
    let gone = lib.record_open(opened("/nowhere/b.pdf", 2, Format::Pdf, None), 2_000);
    lib.record_position(a, page(12, 100), 2_100);
    save(&folder, &mut lib);

    let loaded = folder.load(DeviceId(99));
    assert!(
        loaded.writable && loaded.notes.is_empty(),
        "{:?}",
        loaded.notes
    );
    let back = loaded.library;
    // The device is the library's own, not the one asked for.
    assert_eq!(back.device(), DEVICE);
    assert_eq!(back.len(), 2);
    assert_eq!(back.position(a), Some(page(12, 100)));
    assert_eq!(back.book_at(&book_file), Some(a));
    assert!(back.book(gone).unwrap().missing());
    assert!(!back.book(a).unwrap().missing());
}

#[test]
fn a_newer_library_is_left_alone_and_a_broken_one_kept_aside() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let file = dir.path().join("library.json");
    let newer =
        r#"{"schema":99,"clock":{"last":"0000000000000-00000000-0000000000000007"},"books":[]}"#;
    std::fs::write(&file, newer).unwrap();
    let loaded = folder.load(DEVICE);
    assert!(!loaded.writable);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), newer);

    std::fs::write(&file, "{ not json").unwrap();
    let loaded = folder.load(DEVICE);
    assert!(loaded.writable && loaded.library.is_empty());
    assert_eq!(loaded.notes.len(), 1);
    assert!(dir.path().join("library.unreadable.json").is_file());
    assert!(!file.exists());
}

#[test]
fn text_is_hashed_whole_and_large_books_by_their_ends() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let id = |path: &Path, format| disk::measure(path, format).unwrap().content;
    // A same-length edit in the middle of a note is new content.
    let mut note = vec![b'a'; 300 << 10];
    let pdf = id(&write("p0.pdf", &note), Format::Pdf);
    let before = id(&write("n1.md", &note), Format::Markdown);
    if let Some(middle) = note.get_mut(150 << 10) {
        *middle = b'b';
    }
    let after = id(&write("n2.md", &note), Format::Markdown);
    assert_ne!(before, after);
    // A PDF's middle isn't read; its tail (the xref) changes with edits.
    assert_eq!(id(&write("p1.pdf", &note), Format::Pdf), pdf);
    let mut tail = note.clone();
    if let Some(last) = tail.last_mut() {
        *last = b'z';
    }
    assert_ne!(id(&write("p2.pdf", &tail), Format::Pdf), pdf);
    // Size counts, so a truncated copy differs.
    let short = note.get(..note.len() - 1).unwrap();
    assert_ne!(id(&write("p3.pdf", short), Format::Pdf), pdf);
    let stamp = disk::measure(&dir.path().join("p1.pdf"), Format::Pdf)
        .unwrap()
        .stamp;
    assert_eq!(stamp.size, 300 << 10);
}

mod view {
    use std::cmp::Ordering;

    use super::*;
    use crate::{SortKey, View, natural};

    fn titled(lib: &mut Library, title: &str, author: Option<&str>, at: u64) -> BookId {
        let content = u128::from(at);
        let mut o = opened(&format!("/{at}.pdf"), content, Format::Pdf, None);
        o.title = title.into();
        o.author = author.map(Into::into);
        lib.record_open(o, at)
    }

    fn order(lib: &Library, view: &View) -> Vec<String> {
        let books = view.sort(lib.books());
        books.iter().map(|b| b.title.value.clone()).collect()
    }

    #[test]
    fn numbers_in_names_sort_by_value_ignoring_case() {
        assert_eq!(natural("Volume 2", "Volume 10"), Ordering::Less);
        assert_eq!(natural("file007", "file7"), Ordering::Equal);
        assert_eq!(natural("apple", "Banana"), Ordering::Less);
        assert_eq!(natural("Éclair", "éclair"), Ordering::Equal);
        assert_eq!(natural("  Raven", "raven"), Ordering::Equal);
        assert_eq!(natural("Part 3b", "Part 3a"), Ordering::Greater);
        assert_eq!(natural("9", "10 Lives"), Ordering::Less);
        assert_eq!(natural("", "a"), Ordering::Less);
    }

    #[test]
    fn the_default_is_arranged_and_last_read_starts_newest_first() {
        let mut lib = Library::new(DEVICE);
        titled(&mut lib, "Old", None, 1_000);
        titled(&mut lib, "New", None, 3_000);
        titled(&mut lib, "Mid", None, 2_000);
        let mut view = View::default();
        assert_eq!(view.sort, SortKey::Arranged);
        view.set_sort(SortKey::Arranged);
        assert!(view.ascending, "Arranged has one way round");
        view.set_sort(SortKey::LastRead);
        assert_eq!(order(&lib, &view), ["New", "Mid", "Old"]);
        view.set_sort(SortKey::LastRead);
        assert!(view.ascending, "the same key flips");
        assert_eq!(order(&lib, &view), ["Old", "Mid", "New"]);
    }

    #[test]
    fn titles_sort_naturally_and_a_new_key_starts_in_its_direction() {
        let mut lib = Library::new(DEVICE);
        titled(&mut lib, "Volume 10", None, 1_000);
        titled(&mut lib, "volume 2", None, 2_000);
        titled(&mut lib, "Atlas", None, 3_000);
        let mut view = View::default();
        view.set_sort(SortKey::Title);
        assert!(view.ascending);
        assert_eq!(order(&lib, &view), ["Atlas", "volume 2", "Volume 10"]);
        view.set_sort(SortKey::Title);
        assert_eq!(order(&lib, &view), ["Volume 10", "volume 2", "Atlas"]);
    }

    #[test]
    fn books_without_an_author_sort_last_both_ways() {
        let mut lib = Library::new(DEVICE);
        titled(&mut lib, "Nobody", None, 1_000);
        titled(&mut lib, "Blank", Some("  "), 2_000);
        titled(&mut lib, "By Zola", Some("Zola"), 3_000);
        titled(&mut lib, "By Austen", Some("austen"), 4_000);
        let mut view = View::default();
        view.set_sort(SortKey::Author);
        assert_eq!(
            order(&lib, &view),
            ["By Austen", "By Zola", "Blank", "Nobody"]
        );
        view.set_sort(SortKey::Author);
        assert_eq!(
            order(&lib, &view),
            ["By Zola", "By Austen", "Blank", "Nobody"]
        );
    }

    #[test]
    fn equal_keys_fall_back_to_title_then_id() {
        let mut lib = Library::new(DEVICE);
        let a = titled(&mut lib, "Same", None, 1_000);
        let b = titled(&mut lib, "Same", None, 2_000);
        let mut view = View::default();
        view.set_sort(SortKey::Author);
        let ids: Vec<BookId> = view.sort(lib.books()).iter().map(|b| b.id).collect();
        assert_eq!(ids, [a.min(b), a.max(b)]);
    }

    #[test]
    fn pinned_columns_stay_in_range() {
        let mut view = View {
            columns: Some(40),
            ..View::default()
        };
        view.sanitize();
        assert_eq!(view.columns, Some(crate::COLUMNS_MAX));
        view.columns = Some(0);
        view.sanitize();
        assert_eq!(view.columns, Some(crate::COLUMNS_MIN));
    }

    #[test]
    fn an_author_fills_in_once_and_is_kept() {
        let mut lib = Library::new(DEVICE);
        let id = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
        let mut named = opened("/a.pdf", 1, Format::Pdf, None);
        named.author = Some("Poe".into());
        lib.record_open(named.clone(), 2_000);
        let author = |lib: &Library| lib.book(id).and_then(|b| b.author.value.clone());
        assert_eq!(author(&lib).as_deref(), Some("Poe"));
        named.author = Some("Other".into());
        lib.record_open(named, 3_000);
        assert_eq!(author(&lib).as_deref(), Some("Poe"));
    }
}
