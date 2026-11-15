use crate::disk::Folder;
use crate::test_support::opened;
use crate::{BookId, DeviceId, Format, Item, Library, ShelfError, ShelfId, ShelfRemoved};

const DEVICE: DeviceId = DeviceId(7);

/// A library holding books 1 to `count`.
fn library(count: u128) -> (Library, Vec<BookId>) {
    let mut lib = Library::new(DEVICE);
    let ids = (1..=count)
        .map(|n| {
            lib.record_open(
                opened(&format!("/books/{n}.pdf"), n, Format::Pdf, None),
                n as u64,
            )
        })
        .collect();
    lib.take_dirty();
    (lib, ids)
}

/// Moves `book` before `before` on `shelf`, as a drag does.
fn put(lib: &mut Library, shelf: ShelfId, book: BookId, before: Option<BookId>, at: u64) -> bool {
    let before = before.map(Item::Book);
    lib.arrange(Some(shelf), &[Item::Book(book)], before, at)
}

fn names(lib: &Library, parent: Option<ShelfId>) -> Vec<String> {
    let shelves = lib.children(parent).into_iter();
    shelves.map(|s| s.name.value.clone()).collect()
}

fn on(lib: &Library, shelf: ShelfId) -> Vec<BookId> {
    lib.shelf_books(shelf).iter().map(|b| b.id).collect()
}

#[test]
fn shelves_nest_sort_by_name_and_may_share_a_name() {
    let (mut lib, _) = library(0);
    let fiction = lib.create_shelf("  Fiction\n", None, 10).unwrap();
    assert_eq!(
        lib.take_dirty().shelves.into_iter().collect::<Vec<_>>(),
        [fiction]
    );
    lib.create_shelf("Shelf 10", None, 11).unwrap();
    lib.create_shelf("shelf 9", None, 12).unwrap();
    assert_eq!(names(&lib, None), ["Fiction", "shelf 9", "Shelf 10"]);
    // Two shelves may share a name, as two folders on disk can't.
    let twin = lib.create_shelf("FICTION", None, 13).unwrap();
    assert_ne!(twin, fiction);
    lib.remove_shelf(twin, 13);
    assert_eq!(lib.create_shelf(" \t ", None, 13), Err(ShelfError::Blank));
    // The same name may sit one level down.
    let inner = lib.create_shelf("Fiction", Some(fiction), 14).unwrap();
    let path: Vec<ShelfId> = lib.path_to(inner).iter().map(|s| s.id).collect();
    assert_eq!(path, [fiction, inner]);
    assert_eq!(lib.counts(fiction), (0, 1));
    let long = "x".repeat(500);
    let id = lib.create_shelf(&long, None, 15).unwrap();
    assert_eq!(lib.shelf(id).unwrap().name.value.len(), crate::NAME_MAX);
}

#[test]
fn moves_refuse_loops_and_names_may_repeat() {
    let (mut lib, _) = library(0);
    let a = lib.create_shelf("A", None, 10).unwrap();
    let b = lib.create_shelf("B", Some(a), 11).unwrap();
    let c = lib.create_shelf("C", Some(b), 12).unwrap();
    lib.create_shelf("C", None, 13).unwrap();
    assert_eq!(lib.move_shelf(a, Some(c), 14), Err(ShelfError::Inside));
    assert_eq!(lib.move_shelf(a, Some(a), 14), Err(ShelfError::Inside));
    lib.move_shelf(c, None, 14).unwrap();
    assert_eq!(names(&lib, None), ["A", "C", "C"]);
    lib.move_shelf(c, Some(a), 15).unwrap();
    assert_eq!(names(&lib, Some(a)), ["B", "C"]);
    lib.rename_shelf(b, "c", 16).unwrap();
    let mut both = names(&lib, Some(a));
    both.sort();
    assert_eq!(both, ["C", "c"]);
    assert_eq!(lib.rename_shelf(ShelfId(1), "x", 18), Err(ShelfError::Gone));
}

#[test]
fn removing_a_shelf_keeps_its_books_and_moves_its_shelves_up() {
    let (mut lib, ids) = library(2);
    let top = lib.create_shelf("Notes", None, 10).unwrap();
    let doomed = lib.create_shelf("Old", None, 11).unwrap();
    lib.create_shelf("Notes", Some(doomed), 12).unwrap();
    lib.create_shelf("Drafts", Some(doomed), 13).unwrap();
    for &id in &ids {
        assert!(lib.file(id, doomed, 14));
    }
    let done = lib.remove_shelf(doomed, 15).unwrap();
    assert_eq!(
        done,
        ShelfRemoved {
            books: 2,
            shelves: 2,
        }
    );
    assert!(lib.shelf(doomed).is_none());
    assert_eq!(names(&lib, None), ["Drafts", "Notes", "Notes"]);
    assert_eq!(lib.books().count(), 2);
    assert!(lib.shelved().is_empty());
    assert!(lib.shelves_of(ids[0]).is_empty());
    assert_eq!(lib.counts(top), (0, 0));
    assert!(lib.remove_shelf(doomed, 16).is_none());
}

#[test]
fn filing_appends_once_and_a_book_can_sit_on_many_shelves() {
    let (mut lib, ids) = library(3);
    let a = lib.create_shelf("A", None, 10).unwrap();
    let b = lib.create_shelf("B", None, 11).unwrap();
    assert!(lib.file(ids[2], a, 12));
    assert!(lib.file(ids[0], a, 13));
    assert!(!lib.file(ids[0], a, 14), "already on it");
    assert!(lib.file(ids[0], b, 15));
    assert_eq!(on(&lib, a), [ids[2], ids[0]]);
    assert_eq!(lib.shelves_of(ids[0]).len(), 2);
    assert!(!lib.file(BookId(999), a, 16), "no such book");
    assert!(lib.unfile(ids[2], a, 17));
    assert!(!lib.unfile(ids[2], a, 18));
    assert_eq!(on(&lib, a), [ids[0]]);
    // Filed again, it goes to the end, not its old place.
    lib.file(ids[1], a, 19);
    lib.file(ids[2], a, 20);
    assert_eq!(on(&lib, a), [ids[0], ids[1], ids[2]]);
    let shelved: Vec<BookId> = lib.shelved().into_iter().collect();
    assert_eq!(shelved.len(), 3);
}

#[test]
fn a_book_moves_anywhere_in_a_shelfs_order() {
    let (mut lib, ids) = library(4);
    let s = lib.create_shelf("S", None, 10).unwrap();
    for (n, &id) in ids.iter().enumerate() {
        lib.file(id, s, 11 + n as u64);
    }
    assert!(put(&mut lib, s, ids[3], Some(ids[0]), 20));
    assert_eq!(on(&lib, s), [ids[3], ids[0], ids[1], ids[2]]);
    assert!(put(&mut lib, s, ids[3], Some(ids[2]), 21));
    assert_eq!(on(&lib, s), [ids[0], ids[1], ids[3], ids[2]]);
    assert!(put(&mut lib, s, ids[0], None, 22));
    assert_eq!(on(&lib, s), [ids[1], ids[3], ids[2], ids[0]]);
    assert!(!put(&mut lib, s, ids[0], Some(ids[0]), 23));
    // Many drops into one gap still keep the order.
    for n in 0..200 {
        let (moving, before) = if n % 2 == 0 { (1, 2) } else { (2, 1) };
        put(&mut lib, s, ids[moving], Some(ids[before]), 30 + n);
    }
    let order = on(&lib, s);
    assert_eq!(order.len(), 4);
    assert_eq!(order.last(), Some(&ids[0]));
}

#[test]
fn a_shelf_whose_parent_went_in_a_merge_is_top_level() {
    let (mut lib, _) = library(0);
    let a = lib.create_shelf("A", None, 10).unwrap();
    let b = lib.create_shelf("B", Some(a), 11).unwrap();
    // As a sync can leave: a parent that loops back, or is unknown.
    lib.shelves.get_mut(&a).unwrap().parent.value = Some(b);
    assert!(lib.path_to(a).is_empty());
    assert!(lib.children(None).is_empty());
    lib.shelves.get_mut(&a).unwrap().parent.value = Some(ShelfId(5));
    assert_eq!(names(&lib, None), ["A"]);
    assert_eq!(lib.path_to(b).len(), 2);
}

#[test]
fn shelves_and_their_order_come_back_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let (mut lib, ids) = library(2);
    let a = lib.create_shelf("A", None, 10).unwrap();
    let b = lib.create_shelf("B", Some(a), 11).unwrap();
    lib.file(ids[1], b, 12);
    lib.file(ids[0], b, 13);
    put(&mut lib, b, ids[0], Some(ids[1]), 14);
    let gone = lib.create_shelf("Gone", None, 15).unwrap();
    lib.remove_shelf(gone, 16);
    // The books were taken as saved: write them too.
    lib.mark_all();
    crate::tests::save(&folder, &mut lib);
    let back = folder.load(DEVICE).library;
    assert_eq!(names(&back, None), ["A"]);
    assert_eq!(names(&back, Some(a)), ["B"]);
    assert_eq!(on(&back, b), [ids[0], ids[1]]);
    // The tombstone is kept, so a merge can't bring the shelf back.
    assert!(back.shelves.get(&gone).is_some_and(|s| !s.alive()));
}

#[test]
fn a_shelf_taken_apart_lets_its_books_go_and_a_rung_sends_them_up() {
    let (mut lib, ids) = library(1);
    let outer = lib.create_shelf("Outer", None, 10).unwrap();
    let inner = lib.create_shelf("Inner", Some(outer), 11).unwrap();
    lib.file(ids[0], inner, 12);
    lib.remove_shelf(inner, 13).unwrap();
    assert!(
        lib.shelves_of(ids[0]).is_empty(),
        "not onto the shelf above"
    );
    let mut lib = Library::new(DEVICE);
    let files = [
        crate::run_tests::found("/books/a.pdf", 1),
        crate::run_tests::found("/books/sf/deep/b.pdf", 2),
    ];
    crate::run_tests::import(&mut lib, "/books", crate::folder::Mode::InPlace, &files);
    let root = crate::run_tests::shelf_named(&lib, "books");
    let sf = crate::run_tests::shelf_named(&lib, "sf");
    let deep = crate::run_tests::shelf_named(&lib, "deep");
    assert_eq!(lib.seat(deep), Some(Some(sf)));
    lib.remove_shelf(deep, 14).unwrap();
    assert_eq!(
        crate::run_tests::titles_on(&lib, sf),
        ["b"],
        "the rung above"
    );
    lib.remove_shelf(sf, 15).unwrap();
    assert_eq!(crate::run_tests::titles_on(&lib, root), ["a", "b"]);
}
