//! Two machines trading rows as a sync will, and what they end up with.

use std::path::Path;

use serde_json::{Value, json};

use crate::disk::Folder;
use crate::test_support::{opened, page, unsent};
use crate::tests::save;
use crate::{Absorbed, Book, BookId, Data, DeviceId, Format, Kind, Library, ShelfId};

const A: DeviceId = DeviceId(1);
const B: DeviceId = DeviceId(2);

/// Trades rows both ways until neither has news; what each mended.
fn sync(a: &mut Library, b: &mut Library, wall_ms: u64) -> (Absorbed, Absorbed) {
    let (mut took_a, mut took_b) = (Absorbed::default(), Absorbed::default());
    for _ in 0..6 {
        let (to_b, to_a) = (unsent(a), unsent(b));
        if to_a.is_empty() && to_b.is_empty() {
            return (took_a, took_b);
        }
        let (got_a, got_b) = (a.absorb(to_a, wall_ms), b.absorb(to_b, wall_ms));
        took_a.unlooped += got_a.unlooped;
        took_a.folded += got_a.folded;
        took_b.unlooped += got_b.unlooped;
        took_b.folded += got_b.folded;
    }
    panic!("the two never settled");
}

fn path(lib: &Library, id: ShelfId) -> Vec<String> {
    let path = lib.path_to(id).into_iter();
    path.map(|s| s.name.value.clone()).collect()
}

/// What a reader sees, to compare two machines by.
fn picture(lib: &Library) -> String {
    let mut out = Vec::new();
    for book in lib.books() {
        let on = lib.shelves_of(book.id).into_iter();
        let on: Vec<String> = on.map(|s| path(lib, s).join("/")).collect();
        let at = lib.position(book.id).map(|p| p.page);
        let twin = book.twin_of.value;
        out.push(format!(
            "{} {:?} {twin:?} {at:?} {on:?}",
            book.id, book.title.value
        ));
    }
    for shelf in lib.every_shelf() {
        out.push(path(lib, shelf.id).join("/"));
    }
    out.join("\n")
}

#[test]
fn edits_on_two_machines_both_land() {
    let mut a = Library::new(A);
    let id = a.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let mut b = Library::new(B);
    sync(&mut a, &mut b, 1_100);
    assert!(b.book(id).is_some());

    a.rename_book(id, "Renamed", 2_000);
    b.record_position(id, page(41, 100), 2_100);
    let shelf = b.create_shelf("Poems", None, 2_200).unwrap();
    b.file(id, shelf, 2_300);
    sync(&mut a, &mut b, 3_000);
    assert_eq!(picture(&a), picture(&b));
    assert_eq!(a.book(id).unwrap().title.value, "Renamed");
    assert_eq!(a.position(id), Some(page(41, 100)));
    assert_eq!(a.shelf_books(shelf).len(), 1);
    // Each clock now passes every stamp it took in.
    let seen = a.book(id).unwrap().title.at;
    assert!(b.clock.tick(0) > seen);
}

#[test]
fn fields_from_a_newer_build_outlive_an_older_one() {
    let mut a = Library::new(A);
    let id = a.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let mut rows = unsent(&mut a);
    let row = rows.iter_mut().find(|r| r.kind == Kind::Book).unwrap();
    let mut body: Value = serde_json::from_str(&row.body).unwrap();
    // A stamp at 2 s, by device 9.
    let at = "00000000007d0-00000000-0000000000000009";
    body["rating"] = json!({ "v": 5, "t": at });
    row.body = body.to_string();

    let mut b = Library::new(B);
    b.absorb(rows, 1_500);
    assert!(b.clock.tick(0).ms >= 2_000);
    // This build doesn't know the field, edits the book and sends it on.
    b.rename_book(id, "Other", 3_000);
    let sent = unsent(&mut b);
    let row = sent.iter().find(|r| r.kind == Kind::Book).unwrap();
    let body: Value = serde_json::from_str(&row.body).unwrap();
    assert_eq!(
        (&body["rating"]["v"], &body["title"]["v"]),
        (&json!(5), &json!("Other"))
    );

    // The store keeps it too.
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    b.absorb(sent, 3_100);
    b.mark_all();
    save(&folder, &mut b);
    let back = folder.load(B).library;
    let rating = back.book(id).unwrap().extra.get("rating").cloned();
    assert_eq!(rating, Some(json!({ "v": 5, "t": at })));
}

/// Shelves P > X and Y, on both machines.
fn shelves() -> (Library, Library, [ShelfId; 3]) {
    let mut a = Library::new(A);
    let p = a.create_shelf("P", None, 1_000).unwrap();
    let x = a.create_shelf("X", Some(p), 1_001).unwrap();
    let y = a.create_shelf("Y", None, 1_002).unwrap();
    let mut b = Library::new(B);
    sync(&mut a, &mut b, 1_100);
    (a, b, [p, x, y])
}

#[test]
fn a_loop_of_shelves_drops_its_newest_move() {
    // X goes into Y here while Y goes into X there, later.
    let (mut a, mut b, [_, x, y]) = shelves();
    a.move_shelf(x, Some(y), 2_000).unwrap();
    b.move_shelf(y, Some(x), 3_000).unwrap();
    let (took_a, took_b) = sync(&mut a, &mut b, 4_000);
    assert!(took_a.unlooped > 0 && took_b.unlooped > 0);
    for lib in [&a, &b] {
        assert_eq!(path(lib, x), ["Y", "X"]);
        assert_eq!(path(lib, y), ["Y"]);
    }
    assert_eq!(picture(&a), picture(&b));

    // The other way round, X goes back where it was before.
    let (mut a, mut b, [_, x, y]) = shelves();
    a.move_shelf(x, Some(y), 3_000).unwrap();
    b.move_shelf(y, Some(x), 2_000).unwrap();
    sync(&mut a, &mut b, 4_000);
    for lib in [&a, &b] {
        assert_eq!(path(lib, x), ["P", "X"]);
        assert_eq!(path(lib, y), ["P", "X", "Y"]);
    }
    assert_eq!(picture(&a), picture(&b));
}

#[test]
fn two_shelves_of_one_name_both_stay_as_named() {
    let mut a = Library::new(A);
    let mut b = Library::new(B);
    let first = a.create_shelf("Poetry", None, 1_000).unwrap();
    let second = b.create_shelf("poetry", None, 2_000).unwrap();
    sync(&mut a, &mut b, 3_000);
    for lib in [&a, &b] {
        assert_eq!(path(lib, first), ["Poetry"]);
        assert_eq!(path(lib, second), ["poetry"]);
    }
    assert_eq!(picture(&a), picture(&b));
}

#[test]
fn one_content_as_two_originals_folds_into_the_lower_id() {
    // Here a file edited in place; there the edited file, imported new.
    let mut a = Library::new(A);
    let kept = a.record_open(opened("/notes.md", 1, Format::Pdf, None), 1_000);
    a.record_open(opened("/notes.md", 2, Format::Pdf, None), 2_000);
    let mut b = Library::new(B);
    let gone = b.record_open(opened("/elsewhere/notes.md", 2, Format::Pdf, None), 2_500);
    let shelf = b.create_shelf("Notes", None, 2_600).unwrap();
    b.file(gone, shelf, 2_700);
    b.record_position(gone, page(41, 100), 2_800);
    assert!(kept < gone);

    let (took_a, took_b) = sync(&mut a, &mut b, 3_000);
    assert!(took_a.folded > 0 && took_b.folded > 0);
    for lib in [&a, &b] {
        assert_eq!(lib.len(), 1);
        let on: Vec<BookId> = lib.shelf_books(shelf).iter().map(|b| b.id).collect();
        assert_eq!(on, [kept]);
        assert_eq!(lib.position(kept), Some(page(41, 100)));
        let folded = lib.books.get(&gone).unwrap();
        assert!(!folded.alive());
        assert_eq!(folded.twin_of.value, Some(kept));
    }
    assert_eq!(b.book_at(Path::new("/elsewhere/notes.md")), Some(kept));
    assert_eq!(picture(&a), picture(&b));
}

#[test]
fn a_twin_promoted_on_one_machine_is_promoted_on_all() {
    let mut a = Library::new(A);
    let original = a.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let twin = a.twin(original, 1_100).unwrap();
    let mut b = Library::new(B);
    sync(&mut a, &mut b, 1_200);
    a.remove_books(&[original], Data::Keep, 2_000);
    sync(&mut a, &mut b, 2_100);
    assert!(b.book(original).is_none());
    assert_eq!(b.book(twin).unwrap().twin_of.value, None);
    assert_eq!(picture(&a), picture(&b));

    // Saved before it had a stamp, `twin_of` reads as never written.
    let mut body = serde_json::to_value(a.books.get(&twin).unwrap()).unwrap();
    body["twin_of"] = json!(original.to_string());
    let old: Book = serde_json::from_value(body).unwrap();
    assert_eq!(old.twin_of.value, Some(original));
    assert_eq!(old.twin_of.at, crate::clock::NEVER);
}
