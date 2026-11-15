use std::path::PathBuf;

use crate::disk::{self, Folder};
use crate::folder::Mode;
use crate::ledger::Recovered;
use crate::run_tests::{found, import, rescan};
use crate::test_support::{measure, opened, page};
use crate::{BookId, ContentId, Data, Design, DeviceId, Format, Library, Origin};

const DEVICE: DeviceId = DeviceId(7);

#[test]
fn a_folder_book_removed_stays_out_until_restored_with_its_place() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/s/a.pdf", 1)];
    let (s, landed) = import(&mut lib, "/s", Mode::InPlace, &files);
    let id = landed.placed[0];
    lib.record_position(id, page(4, 10), 2_000);
    let shelf = lib.create_shelf("Read", None, 2_000).unwrap();
    lib.file(id, shelf, 2_000);
    let receipt = lib.receipt(&[id]);
    let got = (
        receipt.books,
        receipt.positions,
        receipt.spots,
        receipt.copies,
        receipt.hidden,
    );
    assert_eq!(got, (1, 1, 2, 0, 1));
    assert!(receipt.filed.contains(&"Read".to_string()));
    assert_eq!(lib.remove_books(&[id], Data::Keep, 3_000), 1);
    assert!(lib.take_swept().is_empty());
    assert!(lib.book(id).is_none() && lib.book_at("/s/a.pdf".as_ref()).is_none());
    // The file is still there; a rescan leaves it out.
    assert_eq!(rescan(&mut lib, s, &files).total(), 0);
    assert!(lib.is_empty());
    assert_eq!(lib.recoverables(s).len(), 1);
    let back = lib.restore_removed(s, ContentId(1), &files[0], None, 4_000);
    assert_eq!(back, Some(id));
    assert!(lib.recoverables(s).is_empty());
    assert_eq!(lib.position(id), Some(page(4, 10)));
    assert!(
        lib.shelf_books(shelf).is_empty(),
        "a removal forgets its spots"
    );
    assert_eq!(lib.book_at("/s/a.pdf".as_ref()), Some(id));
    let again = lib.restore_removed(s, ContentId(1), &files[0], None, 5_000);
    assert_eq!(again, None);
}

#[test]
fn opening_a_removed_folder_book_brings_it_back_and_lifts_its_tombstone() {
    let mut lib = Library::new(DEVICE);
    let (s, landed) = import(&mut lib, "/s", Mode::InPlace, &[found("/s/a.pdf", 1)]);
    let id = landed.placed[0];
    lib.remove_books(&[id], Data::Keep, 2_000);
    assert!(lib.folder(s).unwrap().tombstone(ContentId(1)).is_some());
    assert_eq!(
        lib.record_open(
            opened("/s/a.pdf", 1, Format::Pdf, Some(measure(1).stamp)),
            3_000,
        ),
        id
    );
    assert!(lib.folder(s).unwrap().tombstone(ContentId(1)).is_none());
    // Back but off its folder's shelf: offered as a move, not a removal.
    let offered = lib.recoverables(s);
    assert!(matches!(offered[..], [Recovered::Moved { book, .. }] if book == id));
}

#[test]
fn a_twin_reads_separately_and_follows_its_originals_file() {
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(
        opened("/a.pdf", 1, Format::Pdf, Some(measure(1).stamp)),
        1_000,
    );
    lib.record_position(a, page(5, 10), 1_000);
    let shelf = lib.create_shelf("S", None, 1_000).unwrap();
    let other = lib.record_open(
        opened("/b.pdf", 2, Format::Pdf, Some(measure(2).stamp)),
        1_000,
    );
    lib.file(a, shelf, 1_000);
    lib.file(other, shelf, 1_000);
    let t = lib.twin(a, 2_000).unwrap();
    let t3 = lib.twin(t, 2_000).unwrap();
    let title = |id| lib.book(id).unwrap().title.value.clone();
    assert_eq!(
        (title(t), title(t3)),
        ("Book 1 (2)".into(), "Book 1 (3)".into())
    );
    assert_eq!(lib.book(t3).unwrap().twin_of.value, Some(a));
    assert_eq!(lib.position(t), None);
    // Each twin lands right behind the book it was made from.
    let order: Vec<BookId> = lib.shelf_books(shelf).iter().map(|b| b.id).collect();
    assert_eq!(order, [a, t, t3, other]);
    // Paths still name the original; a move reaches every twin.
    assert_eq!(lib.book_at("/a.pdf".as_ref()), Some(a));
    lib.record_open(
        opened("/moved.pdf", 1, Format::Pdf, Some(measure(1).stamp)),
        3_000,
    );
    assert_eq!(lib.book(t).unwrap().path(), Some("/moved.pdf".as_ref()));
    assert_eq!(
        lib.record_open(
            opened("/moved.pdf", 1, Format::Pdf, Some(measure(1).stamp)),
            3_000,
        ),
        a
    );
    // Read as the twin, only the twin was opened.
    lib.record_open_as(
        opened("/moved.pdf", 1, Format::Pdf, Some(measure(1).stamp)),
        9_000,
        Some(t),
    );
    assert_eq!(lib.book(t).unwrap().opened.value, 9_000);
    assert_eq!(lib.book(a).unwrap().opened.value, 3_000);
}

#[test]
fn removing_an_original_hands_its_files_to_a_twin() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/s/a.pdf", 1)];
    let (s, landed) = import(&mut lib, "/s", Mode::InPlace, &files);
    let a = landed.placed[0];
    let t = lib.twin(a, 2_000).unwrap();
    assert_eq!(lib.receipt(&[a]).hidden, 0);
    lib.remove_books(&[a], Data::Keep, 3_000);
    assert!(lib.folder(s).unwrap().ignored.is_empty());
    assert_eq!(lib.book(t).unwrap().twin_of.value, None);
    assert_eq!(lib.book_at("/s/a.pdf".as_ref()), Some(t));
    // The heir is the folder's book now: a rescan adds nothing.
    assert_eq!(rescan(&mut lib, s, &files).total(), 0);
    assert_eq!(lib.len(), 1);
    // Both gone together, the folder leaves the file out.
    let t2 = lib.twin(t, 4_000).unwrap();
    assert_eq!(lib.receipt(&[t, t2]).hidden, 2);
    lib.remove_books(&[t, t2], Data::Keep, 5_000);
    assert_eq!(rescan(&mut lib, s, &files).total(), 0);
    assert!(lib.is_empty());
}

#[test]
fn store_copies_are_deleted_with_the_last_book_reading_them() {
    let mut lib = Library::new(DEVICE);
    let (_, landed) = import(&mut lib, "/s", Mode::Copy, &[found("/s/a.pdf", 1)]);
    let a = landed.placed[0];
    let origin = lib.book(a).unwrap().locations[0].origin;
    assert_eq!(origin, Origin::Store);
    let t = lib.twin(a, 2_000).unwrap();
    assert_eq!(lib.receipt(&[a]).copies, 0);
    lib.remove_books(&[a], Data::Keep, 3_000);
    assert!(lib.take_swept().is_empty());
    assert_eq!(lib.receipt(&[t]).copies, 1);
    lib.remove_books(&[t], Data::Keep, 4_000);
    assert_eq!(lib.take_swept(), [PathBuf::from("/store/a.pdf")]);
}

#[test]
fn renames_are_trimmed_and_blank_ones_refused() {
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(
        opened("/a.pdf", 1, Format::Pdf, Some(measure(1).stamp)),
        1_000,
    );
    assert!(!lib.rename_book(a, "  \n ", 2_000));
    assert!(!lib.rename_book(a, "Book 1", 2_000));
    assert!(lib.rename_book(a, "  Moby\n Dick ", 2_000));
    assert_eq!(lib.book(a).unwrap().title.value, "Moby Dick");
    // A reopen keeps the reader's own name.
    lib.record_open(
        opened("/a.pdf", 1, Format::Pdf, Some(measure(1).stamp)),
        3_000,
    );
    assert_eq!(lib.book(a).unwrap().title.value, "Moby Dick");
}

#[test]
fn a_books_own_design_is_kept_saved_and_cleared() {
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(
        opened("/a.epub", 1, Format::Pdf, Some(measure(1).stamp)),
        1_000,
    );
    assert!(lib.book(a).unwrap().design.is_unset());
    assert!(lib.set_design(a, Some(Design::Off), 2_000));
    assert!(!lib.set_design(a, Some(Design::Off), 2_500));
    lib.record_open(
        opened("/a.epub", 1, Format::Pdf, Some(measure(1).stamp)),
        3_000,
    );
    let book = lib.book(a).unwrap().clone();
    assert_eq!(book.design.value, Some(Design::Off));
    let json = serde_json::to_string(&book).unwrap();
    assert!(json.contains("\"off\""), "{json}");
    let back: crate::Book = serde_json::from_str(&json).unwrap();
    assert_eq!(back.design, book.design);
    // Records written before the choice existed read as following.
    let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
    old.as_object_mut().unwrap().remove("design");
    let old: crate::Book = serde_json::from_value(old).unwrap();
    assert!(old.design.is_unset());
    assert!(lib.set_design(a, None, 4_000));
    assert_eq!(lib.book(a).unwrap().design.value, None);
}

#[test]
fn copies_land_in_the_store_and_only_store_files_are_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path().join("lib"));
    let file = dir.path().join("Dune.pdf");
    std::fs::write(&file, b"%PDF-1.7 dune").unwrap();
    let content = disk::measure(&file, Format::Pdf).unwrap().content;
    let (copy, stamp) = disk::copy_in(&folder.store(), &file, content).unwrap();
    assert!(copy.ends_with(format!("files/{content}/Dune.pdf")));
    assert_eq!(stamp.size, 13);
    // A second import of the same file reuses the copy.
    assert_eq!(
        disk::copy_in(&folder.store(), &file, content).unwrap().0,
        copy
    );
    let outside = [file.clone(), copy.clone()];
    assert_eq!(disk::delete_copies(&folder.store(), &outside), 1);
    assert!(file.is_file() && !copy.exists() && !copy.parent().unwrap().exists());
}

#[test]
fn a_markdown_copy_brings_its_pictures_and_leaves_with_them() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path().join("lib"));
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(notes.join("img")).unwrap();
    let file = notes.join("trip.md");
    std::fs::write(&file, b"![a](img/a.png)").unwrap();
    std::fs::write(notes.join("img/a.png"), b"png a").unwrap();
    std::fs::write(dir.path().join("up.png"), b"png up").unwrap();
    let content = disk::measure(&file, Format::Markdown).unwrap().content;
    let (copy, _) = disk::copy_in(&folder.store(), &file, content).unwrap();
    let pair = |src: PathBuf, kept: &str| (src, PathBuf::from(kept));
    let pictures = [
        pair(notes.join("img/a.png"), "img/a.png"),
        pair(dir.path().join("up.png"), "^/up.png"),
        pair(notes.join("gone.png"), "gone.png"),
        pair(dir.path().join("up.png"), "../escape.png"),
    ];
    disk::copy_beside(&copy, &pictures);
    let beside = copy.parent().unwrap();
    assert_eq!(std::fs::read(beside.join("img/a.png")).unwrap(), b"png a");
    assert_eq!(std::fs::read(beside.join("^/up.png")).unwrap(), b"png up");
    assert!(!beside.join("gone.png").exists());
    // Nothing is written outside the copy's own folder.
    assert!(!beside.parent().unwrap().join("escape.png").exists());
    assert_eq!(
        disk::delete_copies(&folder.store(), std::slice::from_ref(&copy)),
        1
    );
    assert!(!beside.exists());
}

#[test]
fn twins_and_store_copies_come_back_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(
        opened("/a.pdf", 1, Format::Pdf, Some(measure(1).stamp)),
        1_000,
    );
    let t = lib.twin(a, 2_000).unwrap();
    import(&mut lib, "/s", Mode::Copy, &[found("/s/b.pdf", 2)]);
    crate::tests::save(&folder, &mut lib);
    let back = folder.load(DEVICE).library;
    assert_eq!(back.book(t).unwrap().twin_of.value, Some(a));
    assert_eq!(back.book_at("/a.pdf".as_ref()), Some(a));
    let store = back.books().find(|b| b.content == ContentId(2)).unwrap();
    assert_eq!(store.locations[0].origin, Origin::Store);
}

#[test]
fn a_newer_library_that_wont_parse_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("library.json");
    let newer = r#"{"schema":99,"books":"a shape this build can't read"}"#;
    std::fs::write(&file, newer).unwrap();
    let loaded = Folder::new(dir.path()).load(DEVICE);
    assert!(!loaded.writable);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), newer);
}

#[test]
fn a_cascade_takes_the_insides_and_rungs_name_the_books_they_carry() {
    let mut lib = Library::new(DEVICE);
    let files = [
        found("/books/a.pdf", 1),
        found("/books/sf/b.pdf", 2),
        found("/books/sf/deep/c.pdf", 3),
    ];
    import(&mut lib, "/books", Mode::InPlace, &files);
    let shelf =
        |lib: &Library, name: &str| lib.shelves().find(|s| s.name.value == name).unwrap().id;
    let book =
        |lib: &Library, title: &str| lib.books().find(|b| b.title.value == title).unwrap().id;
    let (sf, deep) = (shelf(&lib, "sf"), shelf(&lib, "deep"));
    let (b, c) = (book(&lib, "b"), book(&lib, "c"));
    let apart = lib.removing(&[], &[sf], false);
    assert_eq!((apart.inside_books, apart.inside_shelves), (2, 1));
    assert_eq!(apart.shelves, [sf]);
    assert!(apart.books.is_empty());
    assert_eq!(apart.copies, [b], "only the books standing on it");
    let all = lib.removing(&[], &[sf], true);
    assert_eq!(all.shelves, [deep, sf], "deepest first");
    let mut gone = all.books.clone();
    gone.sort();
    let mut want = vec![b, c];
    want.sort();
    assert_eq!(gone, want);
    assert!(all.copies.is_empty(), "the books go, so nothing is copied");
    assert_eq!(lib.remove_shelves(&[sf], true, Data::Keep, 9), 2);
    assert!(lib.shelf(sf).is_none() && lib.shelf(deep).is_none());
}
