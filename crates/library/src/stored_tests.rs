use std::path::{Path, PathBuf};

use crate::run::Copied;
use crate::run_tests::{DEVICE, found};
use crate::test_support::opened as base_opened;
use crate::{BookId, ContentId, Found, Library, Opened, Origin};

const STORE: &str = "/lib/files";

/// The copy `copy_in` makes: `files/<content>/<name>`.
fn copy_of(f: &Found) -> Copied {
    let name = f.path.file_name().unwrap();
    let path = Path::new(STORE)
        .join(f.measure.content.0.to_string())
        .join(name);
    Copied {
        path,
        measure: f.measure,
    }
}

/// `file` picked, copied in and landed at the top.
fn picked(lib: &mut Library, file: &Found) -> (BookId, PathBuf) {
    let copy = copy_of(file);
    let landed = lib.land_copies(&[(file.clone(), copy.clone())], None, 2);
    (landed[0], copy.path)
}

/// An open of `path`, as the reader records one.
fn store_open(path: &Path, content: u128) -> Opened {
    let file = found(&path.to_string_lossy(), content);
    let mut opened = base_opened(
        &path.to_string_lossy(),
        content,
        file.format,
        Some(file.measure.stamp),
    );
    opened.title = "x".into();
    opened
}

fn home(lib: &Library, id: BookId) -> (Origin, Option<PathBuf>) {
    let home = lib.book(id).unwrap().home().unwrap();
    (home.origin, home.from.clone())
}

#[test]
fn opening_the_library_s_copy_keeps_it_the_library_s() {
    let mut lib = Library::new(DEVICE);
    let file = found("/downloads/a.pdf", 1);
    let (book, path) = picked(&mut lib, &file);
    assert_eq!(lib.record_open(store_open(&path, 1), 5), book);
    assert_eq!(home(&lib, book), (Origin::Store, Some(file.path.clone())));
    assert_eq!(lib.book(book).unwrap().locations.len(), 1);
    assert_eq!(lib.receipt(&[book]).copies, 1, "removing deletes it");
}

#[test]
fn opening_a_kept_copy_stays_with_its_own_book() {
    let mut lib = Library::new(DEVICE);
    let (first, _) = picked(&mut lib, &found("/a/x.pdf", 1));
    let file = found("/b/x.pdf", 1);
    let copy = Copied {
        path: Path::new(STORE).join("1").join("x (2).pdf"),
        measure: file.measure,
    };
    let kept = lib.land_copies(&[(file, copy.clone())], None, 3)[0];
    assert_ne!(kept, first, "a second book of the same content");
    assert_eq!(lib.record_open(store_open(&copy.path, 1), 5), kept);
    assert_eq!(home(&lib, kept).0, Origin::Store);
    assert_eq!(lib.book(first).unwrap().locations.len(), 1);
}

#[test]
fn opening_a_file_by_hand_still_reads_it_in_place() {
    let mut lib = Library::new(DEVICE);
    let book = lib.record_open(store_open(Path::new("/desk/b.pdf"), 2), 5);
    assert_eq!(home(&lib, book), (Origin::Opened, None));
}

#[test]
fn copies_an_open_marked_as_opened_are_the_library_s_again() {
    let mut lib = Library::new(DEVICE);
    let (book, _) = picked(&mut lib, &found("/downloads/a.pdf", 1));
    let by_hand = lib.record_open(store_open(Path::new("/desk/b.pdf"), 2), 3);
    let odd = lib.record_open(store_open(Path::new("/lib/files/c.pdf"), 3), 3);
    for location in lib.books.get_mut(&book).unwrap().locations.iter_mut() {
        location.origin = Origin::Opened;
    }
    let _ = lib.take_dirty();
    assert_eq!(lib.reclaim_store(Path::new(STORE)), 1);
    assert_eq!(home(&lib, book).0, Origin::Store);
    assert!(lib.is_dirty(), "the fix is saved");
    assert_eq!(home(&lib, by_hand).0, Origin::Opened);
    assert_eq!(home(&lib, odd).0, Origin::Opened, "not a copy's shape");
    assert_eq!(lib.reclaim_store(Path::new(STORE)), 0, "once is enough");
    assert_eq!(lib.book(book).unwrap().content, ContentId(1));
}
