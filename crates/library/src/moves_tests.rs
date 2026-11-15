use std::path::{Path, PathBuf};

use crate::folder::Mode;
use crate::run::Copied;
use crate::run_tests::{DEVICE, found, import, shelf_named, titles_on};
use crate::screen::Answer;
use crate::{
    BookId, ContentId, FileStamp, Format, Library, Measure, Move, NewMark, Opened, Origin,
    Placement, Position, ShelfId, ShelfKind, Spot,
};

fn titled(lib: &Library, title: &str) -> BookId {
    lib.books().find(|b| b.title.value == title).unwrap().id
}

fn copy_of(lib: &Library, id: BookId) -> Copied {
    let book = lib.book(id).unwrap();
    let name = book.path().unwrap().file_name().unwrap();
    Copied {
        path: Path::new("/store").join(name),
        measure: Measure {
            content: book.content,
            stamp: FileStamp::default(),
        },
    }
}

/// A book opened from nowhere a folder reads, filed on `shelf`.
fn recorded_on(lib: &mut Library, title: &str, content: u128, shelf: ShelfId) -> BookId {
    let opened = Opened {
        path: PathBuf::from(format!("/loose/{content}.pdf")),
        measure: Measure {
            content: ContentId(content),
            stamp: FileStamp::default(),
        },
        format: Format::Pdf,
        title: title.into(),
        author: None,
    };
    let id = lib.record_open(opened, 1_000);
    lib.rename_book(id, title, 1_000);
    lib.file(id, shelf, 1_000);
    id
}

fn page(page: usize) -> Position {
    Position {
        page,
        pages: 50,
        place: None,
        spine: None,
    }
}

#[test]
fn a_book_leaving_its_folder_shelf_owes_a_copy_and_lands_as_one() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1), found("/books/sf/b.pdf", 2)];
    let (folder, _) = import(&mut lib, "/books", Mode::InPlace, &files);
    let (root, mine) = (
        shelf_named(&lib, "books"),
        lib.create_shelf("Mine", None, 5).unwrap(),
    );
    let a = titled(&lib, "a");
    let mv = Move {
        books: vec![a],
        from: Some(root),
        to: Some(mine),
        ..Move::default()
    };
    assert_eq!(lib.departure(&mv).books, [a]);
    // No copy yet: the book keeps its place.
    assert_eq!(lib.land_move(&mv, &[], 6).books, 0);
    assert_eq!(titles_on(&lib, root), ["a"]);
    assert!(lib.depart(a, &copy_of(&lib, a), 7));
    let home = lib.book(a).unwrap().home().unwrap().clone();
    assert_eq!(home.origin, Origin::Store);
    assert_eq!(home.from.as_deref(), Some(Path::new("/books/a.pdf")));
    let stone = lib.folder(folder).unwrap().tombstone(ContentId(1)).unwrap();
    assert!(stone.moved && stone.returned.is_none());
    assert_eq!(lib.land_move(&mv, &[a], 8).books, 1);
    assert_eq!(titles_on(&lib, mine), ["a"]);
    assert!(titles_on(&lib, root).is_empty());
}

#[test]
fn another_rung_of_its_folder_is_a_departure_and_alt_adding_is_not() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1), found("/books/sf/b.pdf", 2)];
    import(&mut lib, "/books", Mode::InPlace, &files);
    let (root, sf) = (shelf_named(&lib, "books"), shelf_named(&lib, "sf"));
    let a = titled(&lib, "a");
    let mut mv = Move {
        books: vec![a],
        from: Some(root),
        to: Some(sf),
        ..Move::default()
    };
    assert_eq!(lib.departure(&mv).books, [a]);
    mv.keep = true;
    assert!(lib.departure(&mv).is_empty());
    assert_eq!(lib.land_move(&mv, &[], 6).books, 1);
    assert_eq!(titles_on(&lib, sf), ["a", "b"]);
    assert_eq!(titles_on(&lib, root), ["a"]);
}

/// Two "Dune"s: one on `here`, one on `there` and `also`; the second moves.
fn two_dunes() -> (Library, BookId, BookId, [ShelfId; 3]) {
    let mut lib = Library::new(DEVICE);
    let here = lib.create_shelf("Here", None, 1).unwrap();
    let there = lib.create_shelf("There", None, 1).unwrap();
    let also = lib.create_shelf("Also", None, 1).unwrap();
    let kept = recorded_on(&mut lib, "Dune", 1, here);
    let moving = recorded_on(&mut lib, "dune", 2, there);
    lib.file(moving, also, 2);
    (lib, kept, moving, [here, there, also])
}

fn ask_moving(lib: &mut Library, moving: BookId, from: ShelfId, to: ShelfId) -> crate::MoveAsk {
    let mv = Move {
        books: vec![moving],
        from: Some(from),
        to: Some(to),
        ..Move::default()
    };
    let mut moved = lib.land_move(&mv, &[], 3);
    assert_eq!(moved.books, 0, "nothing lands before the answer");
    let ask = moved.asks.pop().unwrap();
    assert_eq!(ask.offers(), Placement::MOVE);
    ask
}

#[test]
fn merge_keeps_the_book_here_with_the_further_place_marks_and_shelves() {
    let (mut lib, kept, moving, [here, there, also]) = two_dunes();
    lib.record_position(kept, page(3), 2);
    lib.record_position(moving, page(9), 2);
    let spot = Spot::Page {
        page: 9,
        rect: [0.0; 4],
    };
    lib.add_mark(moving, NewMark::bookmark("here".into(), spot), 2);
    let ask = ask_moving(&mut lib, moving, there, here);
    assert_eq!(ask.existing, kept);
    assert_eq!(
        lib.answer_move(&ask, Placement::Merge, 4),
        Answer::Landed(kept)
    );
    assert!(lib.book(moving).is_none());
    assert_eq!(lib.position(kept), Some(page(9)));
    assert_eq!(lib.marks(kept).len(), 1);
    assert!(lib.shelves_of(kept).contains(&also));
    assert!(
        !lib.shelves_of(kept).contains(&there),
        "not the level it left"
    );
    assert!(lib.take_swept().is_empty());
}

#[test]
fn replace_drops_the_book_here_and_the_arrival_takes_its_shelves() {
    let (mut lib, kept, moving, [here, there, also]) = two_dunes();
    let other = lib.create_shelf("Other", None, 2).unwrap();
    lib.file(kept, other, 2);
    let ask = ask_moving(&mut lib, moving, there, here);
    assert_eq!(
        lib.answer_move(&ask, Placement::Replace, 4),
        Answer::Landed(moving)
    );
    assert!(lib.book(kept).is_none());
    let on = lib.shelves_of(moving);
    assert!(on.contains(&here) && on.contains(&other) && on.contains(&also));
    assert!(!on.contains(&there));
}

#[test]
fn keep_both_lands_the_arrival_under_the_next_free_name() {
    let (mut lib, kept, moving, [here, there, _]) = two_dunes();
    let ask = ask_moving(&mut lib, moving, there, here);
    assert_eq!(
        lib.answer_move(&ask, Placement::KeepBoth, 4),
        Answer::Landed(moving)
    );
    assert_eq!(lib.book(moving).unwrap().title.value, "dune_1");
    assert_eq!(titles_on(&lib, here), ["Dune", "dune_1"]);
    assert!(titles_on(&lib, there).is_empty());
    assert!(lib.book(kept).is_some());
    let odd = lib.answer_move(&ask, Placement::Open, 5);
    assert_eq!(odd, Answer::Nothing, "an answer not offered does nothing");
}

#[test]
fn a_rung_off_its_seat_departs_as_the_librarys_own_under_a_copy_name() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1), found("/books/sf/b.pdf", 2)];
    let (folder, _) = import(&mut lib, "/books", Mode::InPlace, &files);
    let sf = shelf_named(&lib, "sf");
    let b = titled(&lib, "b");
    let mv = Move {
        shelves: vec![sf],
        to: None,
        ..Move::default()
    };
    let owed = lib.departure(&mv);
    assert_eq!(
        (owed.shelves.as_slice(), owed.carried.as_slice()),
        ([sf].as_slice(), [b].as_slice())
    );
    assert!(
        owed.homeward.is_empty(),
        "the top level is no seat of its tree"
    );
    assert_eq!(lib.land_move(&mv, &[], 5).stayed, [sf]);
    lib.depart(b, &copy_of(&lib, b), 6);
    let moved = lib.land_move(&mv, &[b], 7);
    assert_eq!(moved.shelves, 1);
    let shelf = lib.shelf(sf).unwrap();
    assert_eq!(shelf.kind.value, ShelfKind::Departed);
    assert_eq!(shelf.name.value, "sf_1");
    assert_eq!(shelf.parent.value, None);
    assert_eq!(titles_on(&lib, sf), ["b"]);
    assert!(!lib.folder(folder).unwrap().shelf_map.contains_key("sf"));
}

#[test]
fn a_member_folder_dropped_into_its_tree_can_fold_back_instead() {
    let mut lib = Library::new(DEVICE);
    let (sub, _) = import(
        &mut lib,
        "/root/sub",
        Mode::InPlace,
        &[found("/root/sub/a.pdf", 1)],
    );
    let (tree, _) = import(&mut lib, "/root", Mode::InPlace, &[found("/root/b.pdf", 2)]);
    let (own, root) = (shelf_named(&lib, "sub"), shelf_named(&lib, "root"));
    let mv = Move {
        shelves: vec![own],
        to: Some(root),
        ..Move::default()
    };
    assert_eq!(lib.departure(&mv).homeward, [own]);
    assert_eq!(lib.send_home(&mv, 5), [own]);
    assert!(lib.folder(sub).is_none(), "its ledger joined the tree");
    let kind = ShelfKind::Rung {
        folder: tree,
        key: "sub".into(),
    };
    assert_eq!(lib.shelf(own).unwrap().kind.value, kind);
    assert_eq!(lib.shelf(own).unwrap().parent.value, Some(root));
}

#[test]
fn a_rung_taken_apart_copies_only_the_books_standing_on_it() {
    let mut lib = Library::new(DEVICE);
    let files = [
        found("/books/sf/a.pdf", 1),
        found("/books/sf/deep/b.pdf", 2),
    ];
    import(&mut lib, "/books", Mode::InPlace, &files);
    let sf = shelf_named(&lib, "sf");
    assert_eq!(lib.rung_books(sf, false), [titled(&lib, "a")]);
    assert_eq!(lib.rung_books(sf, true).len(), 2);
    let mine = lib.create_shelf("Mine", None, 3).unwrap();
    assert!(lib.rung_books(mine, true).is_empty(), "no folder reads it");
}

#[test]
fn a_copy_back_on_its_folder_shelf_spends_the_move_out_log() {
    let mut lib = Library::new(DEVICE);
    let (folder, _) = import(
        &mut lib,
        "/books",
        Mode::InPlace,
        &[found("/books/a.pdf", 1)],
    );
    let (root, mine) = (
        shelf_named(&lib, "books"),
        lib.create_shelf("Mine", None, 5).unwrap(),
    );
    let a = titled(&lib, "a");
    let out = Move {
        books: vec![a],
        from: Some(root),
        to: Some(mine),
        ..Move::default()
    };
    lib.depart(a, &copy_of(&lib, a), 6);
    lib.land_move(&out, &[a], 7);
    let back = Move {
        books: vec![a],
        from: Some(mine),
        to: Some(root),
        ..Move::default()
    };
    assert!(lib.departure(&back).is_empty(), "a copy simply moves");
    lib.land_move(&back, &[], 8);
    let stone = lib.folder(folder).unwrap().tombstone(ContentId(1)).unwrap();
    assert_eq!(stone.returned, Some(a));
}

#[test]
fn a_hand_moved_rung_stays_put_until_it_goes_back_to_its_seat() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1), found("/books/sf/b.pdf", 2)];
    import(&mut lib, "/books", Mode::Copy, &files);
    let (root, sf) = (shelf_named(&lib, "books"), shelf_named(&lib, "sf"));
    let mv = Move {
        shelves: vec![sf],
        to: None,
        ..Move::default()
    };
    assert!(lib.departure(&mv).is_empty(), "a copying tree owes no copy");
    assert_eq!(lib.land_move(&mv, &[], 5).shelves, 1);
    assert!(lib.shelf(sf).unwrap().moved.value);
    lib.move_shelf(sf, Some(root), 6).unwrap();
    assert!(!lib.shelf(sf).unwrap().moved.value);
}

#[test]
fn off_to_the_top_a_book_leaves_only_the_shelf_it_left() {
    let (mut lib, _, moving, [_, there, also]) = two_dunes();
    let mv = Move {
        books: vec![moving],
        from: Some(there),
        ..Move::default()
    };
    assert!(lib.departure(&mv).is_empty(), "nothing read in place");
    assert_eq!(lib.land_move(&mv, &[], 3).books, 1);
    let shelves: Vec<ShelfId> = lib.shelves_of(moving).into_iter().collect();
    assert_eq!(shelves, [also], "its other shelf keeps it");
}

#[test]
fn a_link_stands_among_its_level_and_moves_as_a_card() {
    let mut lib = Library::new(DEVICE);
    let poems = lib.create_shelf("Poems", None, 1_000).unwrap();
    let dune = recorded_on(&mut lib, "Dune", 1, poems);
    let link = lib.link_to(crate::LinkTo::Book(dune), None, 2_000).unwrap();
    let top = lib.arranged(None, crate::Home::Unshelved);
    assert_eq!(
        top.last(),
        Some(&crate::Item::Link(link)),
        "a link is a card"
    );
    let mv = Move {
        links: vec![link],
        to: Some(poems),
        before: Some(crate::Item::Book(dune)),
        ..Move::default()
    };
    let moved = lib.land_move(&mv, &[], 3_000);
    assert_eq!(moved.links, 1);
    assert_eq!(lib.link(link).unwrap().level.value, Some(poems));
    let on = lib.arranged(Some(poems), crate::Home::Everything);
    assert_eq!(on, [crate::Item::Link(link), crate::Item::Book(dune)]);
    assert!(
        !lib.arranged(None, crate::Home::Unshelved)
            .contains(&crate::Item::Link(link))
    );
    // A shelf that's gone takes no link.
    let gone = Move {
        links: vec![link],
        to: Some(ShelfId(7)),
        ..Move::default()
    };
    assert_eq!(lib.land_move(&gone, &[], 4_000).links, 0);
}

#[test]
fn a_book_turned_copy_onto_the_librarys_own_may_become_a_link_to_it() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1)];
    let (folder, _) = import(&mut lib, "/books", Mode::InPlace, &files);
    let (root, mine) = (
        shelf_named(&lib, "books"),
        lib.create_shelf("Mine", None, 5).unwrap(),
    );
    let a = titled(&lib, "a");
    let mut own = copy_of(&lib, a);
    own.path = PathBuf::from("/store/own.pdf");
    own.measure.content = ContentId(9);
    let kept = lib.duplicate_book(a, &own, 6).unwrap();
    lib.unfile(kept, root, 7);
    lib.file(kept, mine, 7);
    lib.rename_book(kept, "a", 7);
    let mv = Move {
        books: vec![a],
        from: Some(root),
        to: Some(mine),
        ..Move::default()
    };
    assert!(lib.depart(a, &copy_of(&lib, a), 8));
    let moved = lib.land_move(&mv, &[a], 9);
    let ask = moved.asks.first().unwrap();
    assert_eq!(ask.offers(), Placement::MOVE_KEEPING_BOTH);
    assert_eq!(
        lib.answer_move(ask, Placement::Replace, 10),
        Answer::Nothing
    );
    assert_eq!(
        lib.answer_move(ask, Placement::LinkOnly, 10),
        Answer::Linked
    );
    assert!(lib.book(a).is_none(), "the dragged copy dissolves");
    assert!(lib.book(kept).is_some());
    let link = lib.links().find(|l| l.level.value == Some(mine)).unwrap();
    assert_eq!(link.target, crate::LinkTo::Book(kept));
    let stone = lib.folder(folder).unwrap().tombstone(ContentId(1)).unwrap();
    assert!(stone.moved, "the folder doesn't place it again");
    // A book that was already a copy meets the plain question.
    let b = recorded_on(&mut lib, "b", 20, root);
    let twin = recorded_on(&mut lib, "b", 21, mine);
    let plain = ask_moving(&mut lib, b, root, mine);
    assert_eq!((plain.offers(), plain.existing), (Placement::MOVE, twin));
}
