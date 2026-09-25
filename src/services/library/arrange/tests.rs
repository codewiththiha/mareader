use super::asking::{CopyAnswer, CopyAsk, books_the_rung_takes};
use super::moves::{insert_many, place_many, reorder_root};
use super::shelf_departure::{
    ReturnPath, departing_book_ids, departing_sets, return_path, target_is_family,
};
use super::shelves::delete_shelf;
use super::*;
use std::collections::{BTreeMap, HashSet};

use leptos::prelude::*;

use crate::state::AppState;
use library_core::book::{Row, find_book_mut};
use library_core::folder::Tombstone;
use library_core::shelf::{ALL_SHELF, Shelf};

use library_core::book::{Book, Fingerprint, Origin};
use library_core::folder::WatchedFolder;
use library_core::shelf::ShelfKind;
use reader_core::format::Format;

fn row(id: &str) -> Row {
    library_core::testkit::markdown_row(id)
}

fn list() -> Vec<Row> {
    vec![row("a"), row("b"), row("c"), row("d")]
}

fn ids(rows: &[Row]) -> Vec<&str> {
    rows.iter().map(|r| r.id()).collect()
}

fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_drop_on_the_root_puts_one_row_where_the_reader_pointed() {
    let mut rows = list();
    reorder_root(&mut rows, &owned(&["d"]), Some(1));
    assert_eq!(ids(&rows), vec!["a", "d", "b", "c"]);
}

#[test]
fn the_index_counts_the_list_as_it_was_before_the_lift() {
    // The reader pointed at the slot "d" occupied while they were holding the two, not two further down the list the lift just shortened.
    let mut rows = list();
    reorder_root(&mut rows, &owned(&["a", "b"]), Some(3));
    assert_eq!(ids(&rows), vec!["c", "a", "b", "d"]);
}

#[test]
fn a_drop_past_the_end_appends() {
    let mut rows = list();
    reorder_root(&mut rows, &owned(&["a"]), Some(99));
    assert_eq!(ids(&rows), vec!["b", "c", "d", "a"]);
}

#[test]
fn an_append_keeps_the_payload_s_order_not_the_list_s() {
    // A set has no order, so the payload is sorted into the level's own order — and the sort is by position in `row_ids`, because putting them back in the list's order would be a drop that quietly shuffled the hand.
    let mut rows = list();
    reorder_root(&mut rows, &owned(&["c", "a"]), None);
    assert_eq!(ids(&rows), vec!["b", "d", "c", "a"]);
}

#[test]
fn a_row_the_list_does_not_hold_is_not_invented() {
    let mut rows = list();
    reorder_root(&mut rows, &owned(&["gone", "b"]), Some(0));
    assert_eq!(ids(&rows), vec!["b", "a", "c", "d"]);
}

#[test]
fn a_link_is_reordered_by_its_own_id_like_any_other_row() {
    let mut rows = vec![
        row("a"),
        Row::link("l1".into(), "Dune".into(), "a".into(), 1),
        row("b"),
    ];
    reorder_root(&mut rows, &owned(&["l1"]), Some(0));
    assert_eq!(ids(&rows), vec!["l1", "a", "b"]);
}

#[test]
fn an_empty_set_leaves_the_list_alone() {
    let mut rows = list();
    reorder_root(&mut rows, &[], Some(0));
    assert_eq!(ids(&rows), vec!["a", "b", "c", "d"]);
}

#[test]
fn a_bulk_move_to_the_root_removes_each_clean_row_from_every_shelf() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    state.library.books.set(list());
    state.library.shelves.set(vec![
        own("from", "From", None, &["a", "b"]),
        own("also", "Also", None, &["a", "c"]),
    ]);

    move_many_to_shelf(
        state,
        &owned(&["a", "b"]),
        Some("from".to_string()),
        ALL_SHELF.to_string(),
        Some(4),
    );

    let shelves = state.library.shelves.get_untracked();
    assert!(
        shelves
            .iter()
            .all(|shelf| !shelf.books.iter().any(|id| id == "a" || id == "b")),
        "a move to All uses the same everywhere-removal as a single-row move"
    );
    assert_eq!(
        ids(&state.library.books.get_untracked()),
        vec!["c", "d", "a", "b"]
    );
}

#[test]
fn a_book_already_on_the_shelf_is_moved_not_duplicated() {
    let mut members = owned(&["a", "b", "c"]);
    place_many(&mut members, &owned(&["a"]), Some(2));
    assert_eq!(
        members,
        vec!["b", "a", "c"],
        "one membership, in the slot the drop named"
    );
}

#[test]
fn the_shift_is_counted_per_book_rather_than_for_the_batch() {
    // Counting the batch instead of the members would have landed the three one slot early.
    let mut members = owned(&["a", "x", "c", "y"]);
    place_many(&mut members, &owned(&["a", "b", "c"]), Some(3));
    assert_eq!(members, vec!["x", "a", "b", "c", "y"]);
}

#[test]
fn filing_with_no_index_appends_in_order() {
    let mut members = owned(&["a"]);
    place_many(&mut members, &owned(&["b", "c"]), None);
    assert_eq!(members, vec!["a", "b", "c"]);
}

#[test]
fn each_item_lands_after_the_last_rather_than_all_at_one_place() {
    let mut list: Vec<&str> = vec!["x", "y"];
    insert_many(&mut list, ["a", "b", "c"].into_iter(), Some(1), 0);
    assert_eq!(list, vec!["x", "a", "b", "c", "y"], "not reversed");
}

#[test]
fn an_index_past_the_end_clamps_per_item() {
    let mut list: Vec<&str> = vec!["x"];
    insert_many(&mut list, ["a", "b"].into_iter(), Some(99), 0);
    assert_eq!(list, vec!["x", "a", "b"]);
}

#[test]
fn a_shift_larger_than_the_index_lands_at_the_front() {
    let mut list: Vec<&str> = vec!["x", "y"];
    insert_many(&mut list, ["a"].into_iter(), Some(1), 4);
    assert_eq!(list, vec!["a", "x", "y"]);
}

fn fp(n: u32) -> Fingerprint {
    Fingerprint {
        size: u64::from(n),
        mtime_ms: u64::from(n),
        head_hash: n,
    }
}

fn linked_at(id: &str, path: &str, n: u32) -> Row {
    Row::Book(Book::new(
        id.to_string(),
        fp(n),
        Format::Markdown,
        Origin::Linked {
            src: path.to_string(),
        },
        0,
    ))
}

fn stored_at(id: &str, src: &str, store: &str, n: u32) -> Row {
    Row::Book(Book::new(
        id.to_string(),
        fp(n),
        Format::Markdown,
        Origin::Stored {
            src: Some(src.to_string()),
            store: store.to_string(),
        },
        0,
    ))
}

fn nested(n: u32) -> WatchedFolder {
    WatchedFolder {
        placed: HashSet::from([fp(n)]),
        shelf_map: BTreeMap::from([
            (String::new(), "shelf1".to_string()),
            ("Fiction".to_string(), "shelf2".to_string()),
            ("Fiction/SciFi".to_string(), "shelf3".to_string()),
        ]),
        ..library_core::testkit::watched_folder("f1", "/books")
    }
}

/// Takes the state rather than making one because the `Owner` a signal needs has to outlive the write.
fn set_nested(state: AppState) {
    state.library.folders.set(vec![nested(7)]);
    state
        .library
        .books
        .set(vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)]);
}

#[test]
fn a_drag_to_another_rung_of_the_same_folder_is_a_departure() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    set_nested(state);
    // Reading the tie as the folder's shelf tree instead — "any shelf this folder owns" — left the row
    // linked at an address it had been dragged off.
    assert!(converts_on_move_to(state, "b1", "shelf2"));
    assert!(converts_on_move_to(state, "b1", "shelf1"));
    assert!(converts_on_move_to(state, "b1", "elsewhere"));
}

#[test]
fn a_reorder_on_the_book_s_own_rung_copies_nothing() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    set_nested(state);
    // Re-ordering the books a folder placed, on the rung it placed them on, is the folder's own business.
    assert!(!converts_on_move_to(state, "b1", "shelf3"));
}

#[test]
fn the_root_and_the_reader_s_own_shelves_are_nobody_s_ground() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    set_nested(state);
    assert!(converts_on_move_to(state, "b1", ALL_SHELF));
    assert!(converts_on_move_to(state, "b1", "mine"));
}

#[test]
fn a_rung_the_reader_deleted_is_ground_the_book_has_left() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let mut folder = nested(7);
    folder.shelf_map.remove("Fiction/SciFi");
    state.library.folders.set(vec![folder]);
    state
        .library
        .books
        .set(vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)]);
    assert!(converts_on_move_to(state, "b1", "shelf2"));
    assert!(converts_on_move_to(state, "b1", "shelf3"));
}

#[test]
fn a_rung_that_already_left_its_tree_owes_no_second_copy() {
    let mut shelves = tree();
    shelves.push(library_core::testkit::departed_shelf(
        "moved",
        &[],
        Some("fic"),
    ));
    let folders = vec![reading_folder()];
    assert!(
        !library_core::shelf::departs_on_move(&shelves, &folders, "moved", None),
        "the move that took it off the tree already paid the copy"
    );
}

#[test]
fn a_folder_that_does_not_group_has_one_ground_for_every_file() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let mut folder = nested(7);
    folder.opts.groups = false;
    folder.shelf_map = BTreeMap::from([(String::new(), "flat".to_string())]);
    state.library.folders.set(vec![folder]);
    state
        .library
        .books
        .set(vec![linked_at("b1", "/books/Fiction/SciFi/dune.md", 7)]);
    assert!(!converts_on_move_to(state, "b1", "flat"));
    assert!(converts_on_move_to(state, "b1", "shelf2"));
}

#[test]
fn only_a_linked_book_of_a_reading_folder_owes_the_copy() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let mut copying = nested(7);
    copying.id = "f2".into();
    copying.opts.in_place = false;
    state.library.folders.set(vec![nested(7), copying]);
    state.library.books.set(vec![
        linked_at("b1", "/books/Fiction/SciFi/dune.md", 7),
        stored_at("b2", "/books/Fiction/SciFi/dune.md", "/store/b2.md", 9),
        linked_at("b3", "/elsewhere/loose.md", 11),
    ]);

    assert!(converts_on_move_to(state, "b1", "shelf2"));
    assert!(!converts_on_move_to(state, "b2", "shelf2"));
    assert!(!converts_on_move_to(state, "b3", "shelf2"));
    assert!(!converts_on_move_to(state, "gone", "shelf2"));
}

fn folder_shelf(id: &str, folder_id: &str, rel: &str) -> Shelf {
    Shelf {
        id: id.to_string(),
        name: id.to_string(),
        kind: ShelfKind::Folder {
            folder_id: folder_id.to_string(),
            rel: Some(rel.to_string()),
        },
        books: Vec::new(),
        parent: None,
        manual_parent: false,
    }
}

fn folder_with_moved_log() -> WatchedFolder {
    WatchedFolder {
        placed: HashSet::from([fp(7)]),
        ignored: vec![Tombstone {
            fp: fp(7),
            title: Some("Dune".to_string()),
            format: Format::Markdown,
            last_path: "/books/Fiction/SciFi/dune.md".to_string(),
            shelf_id: Some("shelf3".to_string()),
            removed_ms: 5,
            moved: true,
            returned_row: None,
        }],
        shelf_map: BTreeMap::from([
            ("Fiction".to_string(), "shelf2".to_string()),
            ("Fiction/SciFi".to_string(), "shelf3".to_string()),
        ]),
        ..library_core::testkit::watched_folder("f1", "/books")
    }
}

#[test]
fn a_departure_s_landing_does_not_bind_the_log_it_just_wrote() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let mut rows = vec![stored_at(
        "b1",
        "/books/Fiction/SciFi/dune.md",
        "/store/b1.md",
        9,
    )];
    find_book_mut(&mut rows, "b1").unwrap().title = Some("Dune".to_string());
    state.library.books.set(rows);
    state
        .library
        .shelves
        .set(vec![folder_shelf("shelf2", "f1", "Fiction")]);
    state.library.folders.set(vec![folder_with_moved_log()]);
    let bound = |state: AppState| {
        state
            .library
            .folders
            .with_untracked(|folders| folders[0].ignored[0].returned_row.is_some())
    };

    // The shape of a return without being one: binding here would spend the log on the row that just left.
    move_row(state, "b1", "shelf2", None, Departed::ThisGesture);
    assert!(!bound(state), "a departure is not a return");
    let filed = state
        .library
        .shelves
        .with_untracked(|shelves| shelves[0].books.iter().any(|id| id == "b1"));
    assert!(filed, "and the move itself still happened");

    move_row(state, "b1", "shelf2", None, Departed::No);
    assert!(
        bound(state),
        "a later gesture binds the log to the row by the address they share"
    );
}

fn reading_folder() -> WatchedFolder {
    WatchedFolder {
        placed: HashSet::from([fp(7), fp(8), fp(9), fp(14)]),
        shelf_map: BTreeMap::from([
            (String::new(), "r".to_string()),
            ("Fiction".to_string(), "fic".to_string()),
            ("Fiction/SciFi".to_string(), "sf".to_string()),
        ]),
        ..library_core::testkit::watched_folder("f1", "/books")
    }
}

fn own(id: &str, name: &str, parent: Option<&str>, books: &[&str]) -> Shelf {
    Shelf {
        id: id.to_string(),
        name: name.to_string(),
        kind: ShelfKind::Virtual,
        books: books.iter().map(|b| b.to_string()).collect(),
        parent: parent.map(str::to_string),
        manual_parent: false,
    }
}

fn rung(
    id: &str,
    folder_id: &str,
    rel: Option<&str>,
    parent: Option<&str>,
    books: &[&str],
) -> Shelf {
    Shelf {
        kind: ShelfKind::Folder {
            folder_id: folder_id.to_string(),
            rel: rel.map(str::to_string),
        },
        ..own(id, id, parent, books)
    }
}

fn tree() -> Vec<Shelf> {
    vec![
        rung("r", "f1", None, None, &["top", "shown2"]),
        rung("fic", "f1", Some("Fiction"), Some("r"), &["mid"]),
        rung(
            "sf",
            "f1",
            Some("Fiction/SciFi"),
            Some("fic"),
            &["deep", "shown2", "loose", "kept"],
        ),
        own("mine", "Mine", Some("fic"), &[]),
        own("elsewhere", "Elsewhere", None, &[]),
    ]
}

fn rows() -> Vec<Row> {
    vec![
        linked_at("top", "/books/top.md", 9),
        linked_at("mid", "/books/Fiction/other.md", 8),
        linked_at("deep", "/books/Fiction/SciFi/dune.md", 7),
        linked_at("shown2", "/books/top2.md", 14),
        linked_at("loose", "/loose/x.md", 12),
        stored_at("kept", "/books/Fiction/SciFi/old.md", "/store/kept.md", 13),
    ]
}

#[test]
fn a_departing_rung_carries_the_books_standing_on_the_rungs_it_takes() {
    let shelves = tree();
    let folder = reading_folder();
    let (subtree, rungs) = departing_sets(&shelves, "f1", "sf");
    assert!(subtree.contains("sf"));
    assert!(rungs.contains("sf"));
    let ids = departing_book_ids(&rows(), &shelves, &folder, &rungs, &subtree);
    assert_eq!(
        ids,
        vec!["deep".to_string()],
        "only the book whose OWN rung is the one leaving"
    );
}

#[test]
fn a_book_shown_on_a_departing_rung_keeps_its_link_when_its_ground_stays() {
    let shelves = tree();
    let folder = reading_folder();
    // "shown2" is a member of "sf" below it, but its address stands on the ROOT rung, which is not departing.
    let (subtree, rungs) = departing_sets(&shelves, "f1", "fic");
    assert!(
        subtree.contains("sf"),
        "the subtree rides with the shelf the hand named"
    );
    assert!(
        subtree.contains("mine"),
        "and the reader's own shelf inside it rides too"
    );
    assert!(
        !subtree.contains("r"),
        "the rung above is not part of the ride"
    );
    let ids = departing_book_ids(&rows(), &shelves, &folder, &rungs, &subtree);
    assert_eq!(
        ids,
        vec!["mid".to_string(), "deep".to_string()],
        "the middle rung's own book and the deep one; not the shown one, not the loose one, not the stored one"
    );
    assert!(!ids.iter().any(|id| id == "shown2"));
    assert!(!ids.iter().any(|id| id == "loose"));
    assert!(!ids.iter().any(|id| id == "kept"));
}

#[test]
fn the_ask_names_the_copies_the_level_s_next_free_names() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let mut other = reading_folder();
    other.id = "f2".into();
    other.root = "/more".into();
    other.placed = HashSet::new();
    other.shelf_map = BTreeMap::from([
        (String::new(), "r2".to_string()),
        ("Fiction".to_string(), "fic2".to_string()),
    ]);
    state.library.folders.set(vec![reading_folder(), other]);
    let mut tree = tree();
    tree[1].name = "Fiction".to_string();
    let mut shelves = vec![
        own("to", "To", None, &[]),
        own("held", "Fiction", Some("to"), &[]),
    ];
    shelves.extend(tree);
    shelves.push(Shelf {
        id: "fic2".to_string(),
        name: "Fiction".to_string(),
        kind: ShelfKind::Folder {
            folder_id: "f2".to_string(),
            rel: Some("Fiction".to_string()),
        },
        books: Vec::new(),
        parent: None,
        manual_parent: false,
    });
    state.library.shelves.set(shelves);
    state.library.books.set(rows());

    let ask = CopyAsk::of_shelf(
        state,
        vec!["fic".to_string(), "fic2".to_string()],
        Some("to".to_string()),
        None,
    )
    .expect("two departing shelves are a question");
    assert_eq!(ask.action, "Move shelves");
    assert_eq!(ask.subject, "2 shelves — 2 books read in place");
    assert!(
        ask.lines[0].contains("their 2 books"),
        "the two levels' books are counted together in the one row: {}",
        ask.lines[0]
    );
    assert!(
        ask.lines[0].contains("“Fiction_1”") && ask.lines[0].contains("“Fiction_2”"),
        "and the row promises the names the copies will wear: {}",
        ask.lines[0]
    );
    assert!(
        ask.options
            .iter()
            .all(|one| one.answer != CopyAnswer::WithoutCopies),
        "a reader's own shelf is nobody's family, so the drop owes no way home"
    );
}

/// f1's tree with a displaced member: the shape a removed rung and a subfolder imported on its own leave behind.
fn family_state() -> (Vec<Shelf>, Vec<WatchedFolder>) {
    let mut tree = reading_folder();
    tree.shelf_map.remove("Fiction/SciFi");
    let mut member = reading_folder();
    member.id = "f3".into();
    member.root = "/books/Fiction/SciFi".into();
    member.shelf_map = BTreeMap::from([(String::new(), "s3".to_string())]);
    let shelves = vec![
        rung("r", "f1", None, None, &[]),
        rung("fic", "f1", Some("Fiction"), Some("r"), &[]),
        rung("s3", "f3", None, None, &["deep"]),
        own("mine", "Mine", None, &[]),
    ];
    (shelves, vec![tree, member])
}

#[test]
fn a_displaced_folder_s_root_shelf_goes_home_by_the_fold() {
    let (shelves, folders) = family_state();
    match return_path(&shelves, &folders, "s3") {
        Some(ReturnPath::Reclaim { tree, gone, rel }) => {
            assert_eq!(tree, "f1", "the family the ground belongs to");
            assert_eq!(gone, "f3", "the folder that was reading it on its own");
            assert_eq!(rel, "Fiction/SciFi", "the rung its directory names");
        }
        _ => panic!("the fold is a displaced root shelf's way home"),
    }
    assert!(target_is_family(
        &shelves,
        &folders,
        Some("fic"),
        "/books/Fiction/SciFi"
    ));
    assert!(target_is_family(
        &shelves,
        &folders,
        Some("r"),
        "/books/Fiction/SciFi"
    ));
    assert!(!target_is_family(
        &shelves,
        &folders,
        Some("mine"),
        "/books/Fiction/SciFi"
    ));
    assert!(!target_is_family(
        &shelves,
        &folders,
        None,
        "/books/Fiction/SciFi"
    ));
    assert!(!target_is_family(
        &shelves,
        &folders,
        Some("gone"),
        "/books/Fiction/SciFi"
    ));
}

#[test]
fn an_off_seat_rung_goes_home_by_the_reseat_and_a_seated_one_is_home() {
    let (shelves, folders) = family_state();
    assert!(return_path(&shelves, &folders, "fic").is_none());
    let mut off = shelves.clone();
    off.iter_mut().find(|s| s.id == "fic").unwrap().parent = Some("mine".to_string());
    match return_path(&off, &folders, "fic") {
        Some(ReturnPath::Reseat { seat }) => assert_eq!(seat.as_deref(), Some("r")),
        _ => panic!("the reseat is an off-seat rung's way home"),
    }
    let mut lifted = shelves.clone();
    lifted.iter_mut().find(|s| s.id == "r").unwrap().parent = Some("mine".to_string());
    match return_path(&lifted, &folders, "r") {
        Some(ReturnPath::Reseat { seat }) => assert_eq!(seat, None),
        _ => panic!("the root's seat is the library's own level"),
    }
}

/// `home/root/1st/2nd`: a read-at-place tree with a rung per folder, its books on the lowest one.
fn deep_tree() -> (Vec<Shelf>, Vec<Row>, WatchedFolder) {
    let mut folder = reading_folder();
    folder.shelf_map = BTreeMap::from([
        (String::new(), "root".to_string()),
        ("1st".to_string(), "one".to_string()),
        ("1st/2nd".to_string(), "two".to_string()),
    ]);
    let shelves = vec![
        rung("root", "f1", None, None, &["b0"]),
        rung("one", "f1", Some("1st"), Some("root"), &[]),
        rung("two", "f1", Some("1st/2nd"), Some("one"), &["b1", "b2"]),
    ];
    let books = vec![
        linked_at("b0", "/books/notes.md", 8),
        linked_at("b1", "/books/1st/2nd/a.md", 7),
        linked_at("b2", "/books/1st/2nd/b.md", 9),
    ];
    (shelves, books, folder)
}

fn set_deep_tree(state: AppState) {
    let (shelves, books, folder) = deep_tree();
    state.library.shelves.set(shelves);
    state.library.books.set(books);
    state.library.folders.set(vec![folder]);
}

#[test]
fn taking_a_rung_apart_brings_its_books_up_one_level_inside_the_tree() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    set_deep_tree(state);

    delete_shelf(state, "two");

    let shelves = state.library.shelves.get_untracked();
    assert!(
        shelves.iter().all(|s| s.id != "two"),
        "one level, and only that one"
    );
    let one = shelves
        .iter()
        .find(|s| s.id == "one")
        .expect("the level above it stands");
    assert_eq!(
        one.books,
        vec!["b1".to_string(), "b2".to_string()],
        "the books come up exactly one level"
    );
    assert_eq!(
        state
            .library
            .folder("f1")
            .expect("the row")
            .shelf_map
            .get("1st/2nd"),
        None,
        "and the folder's map lets the rung go"
    );
}

#[test]
fn the_level_below_a_hole_still_hangs_inside_the_tree() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    set_deep_tree(state);

    delete_shelf(state, "one");
    delete_shelf(state, "two");

    let shelves = state.library.shelves.get_untracked();
    let root = shelves
        .iter()
        .find(|s| s.id == "root")
        .expect("the tree's own rung");
    assert_eq!(
        root.books,
        vec!["b0".to_string(), "b1".to_string(), "b2".to_string()],
        "the repro: `2nd` comes up to `1st`, and `1st` comes up to the root"
    );
    assert_eq!(
        shelves.iter().filter(|s| s.is_folder()).count(),
        1,
        "one level left, and no shelf standing beside the tree"
    );
}

#[test]
fn a_level_holding_books_read_in_place_asks_before_it_comes_apart() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    set_deep_tree(state);

    ask_shelf_apart(state, "two");

    let ask = state
        .library
        .copy_ask
        .ask
        .get_untracked()
        .expect("the question is up");
    assert_eq!(ask.action, "Take shelf apart");
    assert!(
        ask.lines[0].contains("The 2 books read in place here"),
        "the sheet counts the books the copies cost: {}",
        ask.lines[0]
    );
    assert!(
        ask.subject.starts_with("“two”"),
        "and it names the level the reader picked"
    );
    assert!(
        ask.options
            .iter()
            .any(|one| one.answer == CopyAnswer::Copy && one.primary),
        "a level read in place comes apart as copies, and the copies are what the button offers"
    );
    assert!(
        state
            .library
            .shelves
            .get_untracked()
            .iter()
            .any(|s| s.id == "two"),
        "nothing is gone before the reader answers"
    );

    answer_copy(state, CopyAnswer::Cancel);

    assert!(
        state.library.copy_ask.ask.get_untracked().is_none(),
        "the sheet closes with the answer"
    );
    assert!(
        state
            .library
            .shelves
            .get_untracked()
            .iter()
            .any(|s| s.id == "two"),
        "and a question the reader backed out of leaves the tree alone"
    );
}

#[test]
fn taking_a_level_apart_leaves_the_levels_below_it_alone() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    state.library.shelves.set(tree());
    state.library.books.set(rows());
    state.library.folders.set(vec![reading_folder()]);

    // `Fiction` holds one book read in place, and the rung below it holds another on a ground of
    // its own: the level coming apart pays for its own book, and the tree keeps answering for the
    // rest — a copy of a book whose rung stays is a copy the reader never asked for.
    let ask = CopyAsk::of_apart(state, "fic").expect("the level read in place is a question");
    assert!(
        ask.lines[0].contains("The book read in place here becomes a copy"),
        "one book, and no other: {}",
        ask.lines[0]
    );
    assert_eq!(
        books_the_rung_takes(state, "fic"),
        vec!["mid".to_string()],
        "asked and answered through one walk, so the sheet and the copies cannot drift"
    );
}

#[test]
fn a_level_the_library_already_stores_comes_apart_without_a_question() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let mut shelves = tree();
    shelves
        .iter_mut()
        .find(|s| s.id == "sf")
        .expect("the lowest rung")
        .books = vec!["loose".to_string(), "kept".to_string()];
    state.library.shelves.set(shelves);
    state.library.books.set(rows());
    state.library.folders.set(vec![reading_folder()]);

    // `kept` is the library's own copy already and `loose` is a file no folder placed here: neither
    // is a book to make a copy of, so nothing is stored twice and nothing is asked.
    ask_shelf_apart(state, "sf");

    assert!(
        state.library.copy_ask.ask.get_untracked().is_none(),
        "nothing to copy, nothing to ask"
    );
    assert!(
        state
            .library
            .shelves
            .get_untracked()
            .iter()
            .all(|s| s.id != "sf")
    );
}

#[test]
fn a_level_with_nothing_read_in_place_asks_nothing() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    set_deep_tree(state);

    // `1st` holds nothing, and the reader's own shelf is nobody's ground: neither leaves a book
    // answering to a folder.
    ask_shelf_apart(state, "one");
    assert!(state.library.copy_ask.ask.get_untracked().is_none());
    let shelves = state.library.shelves.get_untracked();
    assert!(
        shelves.iter().all(|s| s.id != "one"),
        "the empty level comes apart at once"
    );
    assert_eq!(
        shelves
            .iter()
            .find(|s| s.id == "two")
            .expect("the rung below")
            .parent
            .as_deref(),
        Some("root"),
        "and it brings the level below it up inside the tree"
    );

    state.library.shelves.set(tree());
    ask_shelf_apart(state, "mine");
    assert!(state.library.copy_ask.ask.get_untracked().is_none());
    assert!(
        state
            .library
            .shelves
            .get_untracked()
            .iter()
            .all(|s| s.id != "mine")
    );
}

#[test]
fn the_root_rung_has_nothing_above_it_to_come_up_to() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    set_deep_tree(state);

    delete_shelf(state, "root");

    // The tree's own root going leaves no rung of the folder's above its books: they stay in the
    // library at the top level, and the levels below keep the shape the tree gave them.
    let shelves = state.library.shelves.get_untracked();
    assert!(
        shelves.iter().all(|s| !s.books.contains(&"b0".to_string())),
        "the root's books stand on no rung"
    );
    let two = shelves
        .iter()
        .find(|s| s.id == "two")
        .expect("the rung below stands");
    assert_eq!(two.books, vec!["b1".to_string(), "b2".to_string()]);
    assert_eq!(
        two.parent.as_deref(),
        Some("one"),
        "and still hangs inside the tree"
    );
}
