use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use super::claim::{claim_root, root_is_claimed, when_root_is_free};
use super::files::{land_file, screen_content};
use super::folder::{
    mint_walked_row, resolve_folder, returned_memberships, Landing, Minted, Snapshot,
};use super::gate::{covered_shelf, displaced_member, reclaim_rung, run_fold, RootPlan};
use super::replace::{purge_folder_linked_books, replace_rows_of_tree};
use super::restore::{covered_fate, restore_covered_file, CoveredFate};
use super::{rel_of, shelf_name, Asked};
use crate::state::AppState;
use leptos::prelude::*;
use library_core::book::{Book, Fingerprint, Origin, Row};
use library_core::folder::{FolderOpts, Tombstone, WatchedFolder};
use library_core::tracking::TrackingTree;
use library_core::scan::FoundFile;
use library_core::shelf::Shelf;
use reader_core::format::Format;

/// A measured Markdown file: the cover queue skips anything that is not a
/// PDF, so a host test that lands one never starts the wasm render chain.
/// A drop of a file the library already holds is a question before it is a
/// second copy — the content screen, which a folder walk has always had and a
/// loose file never did.
#[test]
fn a_loose_file_whose_content_the_library_holds_asks_instead_of_landing() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    // The book the reader already has: on another shelf, under another name,
    // which is exactly the case a NAME screen cannot see.
    state.library.books.set(vec![Row::Book(Book {
        title: Some("Dune (imported last week)".to_string()),
        origin: Origin::Linked {
            src: "/elsewhere/dune.pdf".to_string(),
        },
        ..library_core::testkit::book("b1")
    })]);
    state.library.shelves.set(vec![library_core::testkit::shelf(
        "s1",
        "Sci-fi",
        &["b1"],
        None,
    )]);

    // The same content, dropped from a different address and under a different
    // name. `found(.., 1)` is `fp_n(1)` — every field 1 — which is the neutral
    // fingerprint `testkit::book` carries, so this file IS the row above by
    // content while sharing nothing with it by name or address.
    let mut found = vec![found("/downloads/DUNE.pdf", 1)];
    let asks = screen_content(state, &mut found, "all");

    assert!(found.is_empty(), "the file did not stay in the walk to land");
    assert_eq!(asks.len(), 1);
    let ask = &asks[0];
    assert!(
        matches!(
            ask.kind,
            crate::services::library::conflict::AskKind::AlreadyHave
        ),
        "the library's own content question, not a folder's ground"
    );
    assert_eq!(ask.existing_id, "b1");
    assert_eq!(
        ask.existing_name, "Dune (imported last week)",
        "the sheet names the book the reader has, not the file they dropped"
    );
    assert_eq!(
        ask.kind.folder_id(),
        None,
        "no folder's ground is involved, so the sheet names the library"
    );
}

#[test]
fn a_loose_file_the_library_does_not_hold_stays_in_the_walk() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    state.library.books.set(vec![library_core::testkit::row("b1")]);
    let mut found = vec![found("/downloads/other.pdf", 2)];
    let asks = screen_content(state, &mut found, "all");
    assert!(asks.is_empty());
    assert_eq!(found.len(), 1, "an ordinary import is no question at all");
}

#[test]
fn a_held_book_whose_address_died_is_not_offered_as_the_one_you_have() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    state.library.books.set(vec![Row::Book(Book {
        missing: true,
        ..library_core::testkit::book("b1")
    })]);
    // `fp_n(1)` again, so the content matches the missing row and the only
    // reason not to ask is that the row's address is dead.
    let mut found = vec![found("/downloads/dune.pdf", 1)];
    let asks = screen_content(state, &mut found, "all");
    assert!(
        asks.is_empty(),
        "a row the reader cannot be taken to is not an answer to the question"
    );
    assert_eq!(found.len(), 1, "so the file stays in the walk");
}

fn found(path: &str, n: u32) -> FoundFile {
    library_core::testkit::found_md(path, n)
}

/// The same file, with the `rel` a walk of `root` would report: the path
/// under the watched root, which is what the rung key is read from. A test
/// whose file sits in a subfolder needs the real thing, because a `rel` of
/// only the file's own name puts every book on the folder's root shelf.
fn found_under(root: &str, path: &str, n: u32) -> FoundFile {
    let rel = library_core::folder::rel_under(path, root)
        .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).to_string());
    FoundFile {
        rel,
        ..found(path, n)
    }
}

fn plain(id: &str) -> Shelf {
    library_core::testkit::plain_shelf(id, &[])
}

#[test]
fn a_file_lands_as_its_own_row_on_the_level_it_was_dropped_on() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    state.library.shelves.set(vec![plain("a"), plain("b")]);
    let file = found("/one/notes.md", 7);

    land_file(state, &file, None, "a", None);
    assert_eq!(state.library.books.get_untracked().len(), 1);

    // The same file, imported onto an unrelated level. Nothing on that
    // level holds the name, so nothing asks — and the answer to nothing
    // asking is a book on that level, not a shrug. Filing the first
    // level's row here instead would leave the reader looking at a shelf
    // that gained nothing they put there, and one removal would take the
    // book off both.
    land_file(state, &file, None, "b", None);
    let rows = state.library.books.get_untracked();
    assert_eq!(rows.len(), 2, "each level gets a book of its own");
    assert!(
        rows[1].book().is_some_and(|b| b.independent),
        "two books of one address keep their own highlights and place"
    );
    let shelves = state.library.shelves.get_untracked();
    assert_eq!(
        shelves.iter().find(|s| s.id == "b").map(|s| s.books.len()),
        Some(1),
        "and the level it was dropped on is the level it landed on"
    );

    // A name the sheet minted always makes a row too, whatever the library
    // holds: "add as new" is an instruction to add a book.
    land_file(
        state,
        &found("/two/other.md", 9),
        Some("other_1".into()),
        "b",
        None,
    );
    assert_eq!(state.library.books.get_untracked().len(), 3);
}

#[test]
fn one_root_is_one_run_at_a_time() {
    let first = claim_root("/books", Asked::Explicitly);
    assert!(first.is_some());
    assert!(
        claim_root("/books", Asked::Explicitly).is_none(),
        "a second walk of the same tree is refused while the first is live"
    );
    assert!(
        claim_root("/other", Asked::Explicitly).is_some(),
        "a different folder is a different run"
    );
    drop(first);
    assert!(
        claim_root("/books", Asked::Explicitly).is_some(),
        "and the release is the run ending, whatever ended it"
    );
}

#[test]
fn an_ask_waits_out_the_rescan_walking_its_folder() {
    // The shape a watched folder's re-import used to break on: a picker closing
    // is a focus event, a focus event walks every watched folder, and the
    // import the picker was opened for arrives to find its own root claimed.
    // Refusing it there is a re-import that returns nothing at all, because the
    // run that was refused is the only one that lifts a tombstone.
    let started = Rc::new(Cell::new(false));
    let walked = started.clone();
    let rescan = claim_root("/queue", Asked::OnFocus);
    assert!(rescan.is_some());
    assert!(
        when_root_is_free("/queue", move || walked.set(true)),
        "a rescan in flight is waited out rather than answered with a refusal"
    );
    assert!(
        root_is_claimed("/queue"),
        "and the wait counts as a run of this root, so a fold stands aside for it"
    );
    assert!(
        !started.get(),
        "waiting is waiting: the ask does not walk over the run in flight"
    );
    assert!(
        !when_root_is_free("/queue", || {}),
        "a second ask behind the first is refused, and does not replace it"
    );
    drop(rescan);
    assert!(
        started.get(),
        "the rescan's release is what starts the ask, on the ledger it wrote back"
    );
}

#[test]
fn a_run_the_reader_started_is_the_one_an_ask_is_refused_by() {
    let mine = claim_root("/taken", Asked::Explicitly);
    assert!(mine.is_some());
    assert!(
        !when_root_is_free("/taken", || {}),
        "a second import of one folder is the refusal the sentence is for"
    );
    drop(mine);
    assert!(
        when_root_is_free("/taken", || {}),
        "and a free root runs the start at once, with nothing queued"
    );
}

/// A folder in an explicit mode, where [`folder`]'s defaults answer every case
/// the same. The watch arrives as the tree's root decision and not as the flag
/// alone, because the flag is now that decision's mirror: a fixture that set
/// only the flag would be a folder this build never writes, and every surface
/// that reads the tree would answer `false` for it.
fn folder_in_mode(id: &str, root: &str, in_place: bool, watch: bool) -> WatchedFolder {
    let mut folder = WatchedFolder {
        opts: FolderOpts {
            in_place,
            watch,
            ..FolderOpts::default()
        },
        ..folder(id, root, &[], Vec::new())
    };
    if watch {
        folder.set_tracking("", true);
    }
    folder
}

/// A shelf of folder `id`'s tree, at the rung its root files onto: the seat
/// every ground in the tree hangs under.
fn standing(shelf_id: &str, folder_id: &str) -> Shelf {
    standing_at(shelf_id, folder_id, None)
}

/// The same shelf at a rung of the tree's own: the shape a removal of the root
/// shelf leaves standing, lifted to the level the root was on.
fn standing_at(shelf_id: &str, folder_id: &str, rel: Option<&str>) -> Shelf {
    library_core::testkit::folder_shelf(shelf_id, shelf_id, folder_id, rel, &[], None)
}

#[test]
fn a_continuation_run_keeps_the_tree_s_own_root_decision() {
    // A re-pick of ground the tree covers arrives as a CONTINUATION, and the
    // sheet's switch answered for the rung the pick named before the run
    // started: the tree's root is not this run's to rewrite. A run that wrote
    // the sheet's options over the root would turn the whole tree on — or off —
    // from a switch that promised to answer for one subfolder alone, which is
    // what the seated-ground force did while the sheet still had a lock.
    let folders = vec![folder_in_mode("f1", "/books", true, true)];
    let shelves = vec![standing("s1", "f1")];
    let reimported = resolve_folder(
        &folders,
        &shelves,
        "/books",
        FolderOpts::default(),
        &RootPlan {
            continuation: Some(("s1".into(), "Books".into())),
            ..RootPlan::default()
        },
    );
    assert!(reimported.tracked(), "the tree keeps watching its own root");
    assert!(
        reimported.opts.watch,
        "and the flag re-mirrors the tree rather than the sheet"
    );
    assert_eq!(reimported.id, "f1", "it is still the same ledger row");
    // An OFF tree's root survives a rung re-pick whose switch is on: the On is
    // the rung's own decision, written before the run, and the root keeps its
    // Off — the subfolder control the single flag could not express.
    let off = vec![folder_in_mode("f2", "/music", true, false)];
    let rung = resolve_folder(
        &off,
        &[],
        "/music",
        FolderOpts {
            watch: true,
            ..FolderOpts::default()
        },
        &RootPlan {
            continuation: Some(("s2".into(), "Music".into())),
            ..RootPlan::default()
        },
    );
    assert!(!rung.tracked(), "the switch answered for the rung, not the root");
    assert!(!rung.opts.watch, "and the flag mirrors the tree it did not rewrite");
}

#[test]
fn a_watched_folder_no_shelf_of_stands_takes_the_sheets_answer() {
    // The shape taking a folder's shelf apart leaves behind: the row keeps
    // watching, but there is nothing on screen the watch could be read on —
    // so the ground is the sheet's, and an import of it with the switch off
    // is how the invisible watch ends rather than a watch stuck for good.
    let folders = vec![folder_in_mode("f1", "/books", true, true)];
    let resolved = resolve_folder(
        &folders,
        &[],
        "/books",
        FolderOpts::default(),
        &RootPlan::default(),
    );
    assert_eq!(resolved.id, "f1", "the ledger row is still the one standing");
    assert!(
        !resolved.opts.watch,
        "the sheet's answer lands on a folder nothing can see"
    );
    // And the same ground with the switch ON re-watches it: the sheet is the
    // reader's again in both directions.
    let asked = resolve_folder(
        &folders,
        &[],
        "/books",
        FolderOpts {
            watch: true,
            ..FolderOpts::default()
        },
        &RootPlan::default(),
    );
    assert!(asked.opts.watch);
}

#[test]
fn the_sheet_s_watch_answer_lands_in_the_tree_and_not_only_on_the_flag() {
    // The flag is what an older build wrote and read; the tree is what every
    // surface reads from here on. A run that wrote the flag alone would leave a
    // folder the sheet asked to watch with an empty tree — and an empty tree
    // tracks nothing, so the folder would never be rescanned again.
    let asked = resolve_folder(
        &[],
        &[],
        "/books",
        FolderOpts {
            watch: true,
            ..FolderOpts::default()
        },
        &RootPlan::default(),
    );
    assert!(asked.tracked(), "the switch became the root rung's decision");
    assert!(asked.tracks_rung("Fiction/SciFi"), "and the tree below inherits it");
    assert!(asked.opts.watch, "with the flag left agreed");

    // The same ground with the switch off is a folder nothing tracks, and the
    // two halves say so together.
    let off = resolve_folder(
        &[],
        &[],
        "/books",
        FolderOpts {
            watch: false,
            ..FolderOpts::default()
        },
        &RootPlan::default(),
    );
    assert!(!off.tracked());
    assert!(!off.opts.watch);

    // A copying run writes no root decision at all: the sheet does not offer the
    // watch beside a copy, so there is no answer of the reader's to record, and a
    // watched copy would be a folder no surface can turn off.
    let copies = resolve_folder(
        &[],
        &[],
        "/dvds",
        FolderOpts {
            in_place: false,
            watch: true,
            ..FolderOpts::default()
        },
        &RootPlan::default(),
    );
    assert!(!copies.tracked(), "no rung decision was written");
    assert!(copies.opts.watch, "and the flag stays the sheet's, as it always did");
}

#[test]
fn a_rung_left_standing_by_a_removal_leaves_the_ground_to_the_sheet() {
    // What taking a watched folder's ROOT shelf apart leaves behind: the shelves
    // inside it are lifted to the level it was on and still stand, and the map's
    // pointer at the root is the one the removal cut. The freed ground takes the
    // sheet's answer in both directions, and so does every ground beside it: the
    // sheet opens seeded with the tree's own answer for the ground it is on, so
    // what lands is the reader's rather than a stale default — and a rung left
    // hanging is a fold the gate arranges, not a force this resolver owes.
    let folders = vec![folder_in_mode("f1", "/books", true, true)];
    let lifted = vec![standing_at("s1", "f1", Some("scifi"))];
    let freed = resolve_folder(
        &folders,
        &lifted,
        "/books",
        FolderOpts::default(),
        &RootPlan::default(),
    );
    assert_eq!(freed.id, "f1", "the ledger row is still the one standing");
    assert!(
        !freed.tracked(),
        "an import with the switch off ends the watch a removal freed"
    );
    // The same ground with the switch on re-watches it.
    let asked = resolve_folder(
        &folders,
        &lifted,
        "/books",
        FolderOpts {
            watch: true,
            ..FolderOpts::default()
        },
        &RootPlan::default(),
    );
    assert!(asked.tracked(), "and the switch on re-watches it");
    // Ground the lifted rung names, ground under it, and ground beside it all
    // answer the same way: the sheet's.
    for ground in ["/books/scifi", "/books/scifi/deep", "/books/poetry"] {
        let resolved = resolve_folder(
            &folders,
            &lifted,
            ground,
            FolderOpts::default(),
            &RootPlan::default(),
        );
        assert!(!resolved.opts.watch, "{ground} takes the sheet's answer");
    }
}

#[test]
fn the_watch_on_ground_nothing_watches_is_the_sheets_to_set() {
    // An unwatched read-at-place folder: the switch is the reader's, in both
    // directions, and the shelf's own menu is the other hand that sets it.
    let off = resolve_folder(
        &[],
        &[],
        "/books",
        FolderOpts::default(),
        &RootPlan::default(),
    );
    assert!(!off.opts.watch);
    let asked = resolve_folder(
        &[],
        &[],
        "/books",
        FolderOpts {
            watch: true,
            ..FolderOpts::default()
        },
        &RootPlan::default(),
    );
    assert!(asked.opts.watch, "a first import is watched because the sheet said so");
    let standing_folder = vec![folder_in_mode("f1", "/books", true, false)];
    let again = resolve_folder(
        &standing_folder,
        &[standing("s1", "f1")],
        "/books",
        FolderOpts {
            watch: true,
            ..FolderOpts::default()
        },
        &RootPlan::default(),
    );
    assert!(again.opts.watch, "and a re-import can turn a watch on");
    // A COPYING folder's watch is nobody's lock: the sheet does not offer the
    // watch beside a copy, so there is no locked switch for a run to honour.
    let copying = vec![folder_in_mode("f2", "/copies", false, true)];
    let resolved = resolve_folder(
        &copying,
        &[standing("s2", "f2")],
        "/copies",
        FolderOpts {
            in_place: false,
            watch: false,
            ..FolderOpts::default()
        },
        &RootPlan::default(),
    );
    assert!(!resolved.opts.watch);
    // The exemption's own case: a watched read-at-place tree, re-imported as
    // copies. The watch belongs to the mode the reader is leaving, and a
    // watched copy would be a folder no surface offers a way to turn off. The
    // tree stands, so this is the copies exemption answering and not the
    // standing rule.
    let watched = vec![folder_in_mode("f3", "/books", true, true)];
    let copies = resolve_folder(
        &watched,
        &[standing("s3", "f3")],
        "/books",
        FolderOpts {
            in_place: false,
            watch: false,
            ..FolderOpts::default()
        },
        &RootPlan::default(),
    );
    assert!(!copies.opts.watch, "a copies run is a different mode, not this folder watched harder");
    assert!(!copies.opts.in_place);
}

#[test]
fn a_shelf_is_called_by_its_subfolder_and_the_root_by_its_folder() {
    assert_eq!(shelf_name("scifi", "/Users/me/Books"), "scifi");
    assert_eq!(shelf_name("scifi/deep", "/Users/me/Books"), "deep");
    assert_eq!(shelf_name("", "/Users/me/Books"), "Books");
}

#[test]
fn only_the_root_shelf_has_no_subfolder() {
    assert_eq!(rel_of(""), None);
    assert_eq!(rel_of("scifi").as_deref(), Some("scifi"));
    assert_eq!(rel_of("scifi/deep").as_deref(), Some("scifi/deep"));
}

// -------------------------------------------------------------------
// The read-at-place folder's answer to a loose file.
// -------------------------------------------------------------------

fn fp(n: u32) -> Fingerprint {
    Fingerprint {
        size: u64::from(n),
        mtime_ms: u64::from(n),
        head_hash: n,
    }
}

/// An in-place folder that placed the given fingerprints, with the logs
/// it holds — the ledger half of a read-at-place import.
fn folder(id: &str, root: &str, placed: &[u32], ignored: Vec<Tombstone>) -> WatchedFolder {
    WatchedFolder {
        id: id.to_string(),
        root: root.to_string(),
        opts: FolderOpts::default(),
        placed: placed.iter().copied().map(fp).collect(),
        ignored,
        shelf_map: Default::default(),
        last_seen: Vec::new(),
        scanned_ms: 0,
        tracking: TrackingTree::default(),
    }
}

/// A removal's log for `n`, filed on `shelf` when it was filed on one.
fn stone(n: u32, path: &str, moved: bool, shelf: Option<&str>) -> Tombstone {
    Tombstone {
        fp: fp(n),
        title: Some("Dune".to_string()),
        format: Format::Markdown,
        last_path: path.to_string(),
        shelf_id: shelf.map(str::to_string),
        removed_ms: 5,
        moved,
        returned_row: None,
    }
}

#[test]
fn a_file_an_in_place_folder_holds_is_a_question_not_a_second_link() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let file = found_under("/books", "/books/dune.md", 7);
    state.library.shelves.set(vec![plain("fs"), plain("s")]);
    let mut one = folder("f1", "/books", &[7], Vec::new());
    one.shelf_map.insert(String::new(), "fs".to_string());
    state.library.folders.set(vec![one]);
    // The folder's own book for the file, standing where the folder put
    // it — the row an import of the same file must never duplicate.
    let landed = land_file(state, &file, None, "fs", None);

    match covered_fate(state, &file) {
        CoveredFate::Ask { folder_id, row_id } => {
            assert_eq!(folder_id, "f1", "the folder that placed the file is the one asked about");
            assert_eq!(row_id, landed, "and the question names the book it holds");
        }
        other => panic!("expected the folder's question, got {other:?}"),
    }

    // A COPYING folder's tree is no cover: its books are the library's
    // own copies, and the OS file stays an ordinary import.
    let mut copying = folder("f2", "/books", &[7], Vec::new());
    copying.opts.in_place = false;
    state.library.folders.set(vec![copying]);
    assert!(
        matches!(covered_fate(state, &file), CoveredFate::Ordinary),
        "a copying folder's tree asks nothing"
    );
}

#[test]
fn a_file_a_folder_log_remembers_comes_back_to_the_folders_place() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let file = found_under("/books", "/books/scifi/dune.md", 7);
    state.library.shelves.set(vec![plain("fs"), plain("sub"), plain("s")]);
    let mut one = folder("f1", "/books", &[7], vec![stone(7, "/books/scifi/dune.md", false, Some("fs"))]);
    one.shelf_map.insert(String::new(), "fs".to_string());
    one.shelf_map.insert("scifi".to_string(), "sub".to_string());
    state.library.folders.set(vec![one]);

    let fate = covered_fate(state, &file);
    assert!(
        matches!(fate, CoveredFate::Restore { .. }),
        "the log answers before any question: got {fate:?}"
    );
    let CoveredFate::Restore { folder_id, stone } = fate else {
        unreachable!()
    };

    let id = restore_covered_file(state, &file, &folder_id, &stone);

    let rows = state.library.books.get_untracked();
    assert_eq!(rows.len(), 1, "the folder's book is back, and it is the only book");
    let book = rows[0].book().expect("a book row");
    assert_eq!(book.id, id);
    assert_eq!(book.path(), "/books/scifi/dune.md", "linked — it is the folder's file again");
    assert!(matches!(book.origin, Origin::Linked { .. }));
    assert_eq!(book.title.as_deref(), Some("Dune"), "wearing the name the shelf showed");
    let shelves = state.library.shelves.get_untracked();
    let on = |sid: &str| {
        shelves
            .iter()
            .find(|s| s.id == sid)
            .map(|s| s.books.clone())
            .unwrap_or_default()
    };
    assert_eq!(
        on("fs"),
        vec![id],
        "on the shelf the log remembers — not the one the file was dropped on"
    );
    assert!(on("sub").is_empty() && on("s").is_empty());
    let folders = state.library.folders.get_untracked();
    assert!(folders[0].ignored.is_empty(), "the log is spent by the landing");
    assert!(
        folders[0].placed.contains(&file.fp),
        "and the folder still answers for the file, so no rescan doubles it"
    );
}

#[test]
fn a_moved_out_log_with_no_copy_behind_it_brings_the_linked_book_back() {
    // The move made a copy and the copy has since died (a merge folded
    // it away): the log is unbound, and an import of the OS file is owed
    // a real linked book in the folder's place rather than a highlight.
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let file = found_under("/books", "/books/dune.md", 7);
    state.library.shelves.set(vec![plain("fs")]);
    let mut one = folder("f1", "/books", &[7], vec![stone(7, "/books/dune.md", true, Some("fs"))]);
    one.shelf_map.insert(String::new(), "fs".to_string());
    state.library.folders.set(vec![one]);

    match covered_fate(state, &file) {
        CoveredFate::Restore { folder_id, stone } => {
            assert_eq!(folder_id, "f1");
            assert!(stone.moved, "the log it spends is the moved-out one");
        }
        other => panic!("expected the folder's book to come back, got {other:?}"),
    }
}

#[test]
fn a_living_row_outvotes_a_stale_log_beside_it() {
    // The state the next walk prunes — a log standing while a row reads
    // the address, which a hand-open between the removal and the import
    // is how happens — gets the walk's own answer: the registry speaks
    // before the logs, so the import asks about the book that IS there
    // rather than minting a second linked row over it.
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let file = found_under("/books", "/books/dune.md", 7);
    state.library.shelves.set(vec![plain("fs")]);
    state.library.books.set(vec![linked("b1", "/books/dune.md", 7)]);
    let mut one = folder("f1", "/books", &[7], vec![stone(7, "/books/dune.md", false, Some("fs"))]);
    one.shelf_map.insert(String::new(), "fs".to_string());
    state.library.folders.set(vec![one]);

    assert!(
        matches!(covered_fate(state, &file), CoveredFate::Ask { row_id, .. } if row_id == "b1"),
        "the row that is there is the book the import asks about"
    );
}

#[test]
fn a_file_no_in_place_tree_answers_for_is_an_ordinary_import() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    state.library.shelves.set(vec![plain("fs")]);
    let mut one = folder("f1", "/books", &[7], Vec::new());
    one.shelf_map.insert(String::new(), "fs".to_string());
    state.library.folders.set(vec![one]);

    // Under the tree, but a file the folder never placed — new since the
    // last walk, or outside its filters. No book to show and no log to
    // spend: an ordinary import, and the folder places its own linked
    // book on the walk that finds it.
    let fresh = found("/books/new.md", 9);
    assert!(matches!(covered_fate(state, &fresh), CoveredFate::Ordinary));
    // And outside every tree altogether.
    let outside = found("/elsewhere/notes.md", 8);
    assert!(matches!(covered_fate(state, &outside), CoveredFate::Ordinary));
    // A subdirectory spelling of the same fact: "/books2" is not inside
    // "/books", however much the prefix looks like it.
    let neighbour = found("/books2/dune.md", 7);
    assert!(matches!(covered_fate(state, &neighbour), CoveredFate::Ordinary));
    // A file the folder never placed is an ordinary import even with the
    // library's own copy of it standing: the copy's provenance is not the
    // folder's membership, and only the folder's ledger can say the book
    // was once its own.
    state.library.books.set(vec![stored("b9", "/books/new.md", "/store/b9.md", 9)]);
    assert!(
        matches!(covered_fate(state, &fresh), CoveredFate::Ordinary),
        "a copy of a file the folder never placed makes it no less ordinary"
    );
}

/// The departure whose log an older build dropped.
///
/// A host that stamps a copy like its source left the library holding the
/// SOURCE's fingerprint on the copy's row, so the next walk's prune read the
/// moved-out log as a book come back and dropped it. What is left is a folder
/// that placed the file, no row at its address, and no log — and the answer
/// is the one the log would have given: the book comes back as the folder's
/// own linked book, on the folder's own rung, wearing the name the copy
/// carries, and the copy stays where the reader put it.
#[test]
fn a_departure_whose_log_is_gone_still_brings_the_linked_book_back() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let file = found_under("/books", "/books/scifi/dune.md", 7);
    state.library.shelves.set(vec![plain("fs"), plain("mid"), plain("sub")]);
    // The copy the departure made, standing on the middle rung, wearing the
    // source's fingerprint the way a stamp-preserving host leaves it.
    let mut copy = stored("b1", "/books/scifi/dune.md", "/store/b1.md", 7);
    copy.as_book_mut().expect("a book").title = Some("Dune".to_string());
    state.library.books.set(vec![copy]);
    state.library.shelves.update(|shelves| {
        if let Some(shelf) = shelves.iter_mut().find(|s| s.id == "mid") {
            shelf.books.push("b1".to_string());
        }
    });
    let mut one = folder("f1", "/books", &[7], Vec::new());
    one.shelf_map.insert(String::new(), "fs".to_string());
    one.shelf_map.insert("scifi".to_string(), "sub".to_string());
    state.library.folders.set(vec![one]);

    let fate = covered_fate(state, &file);
    assert!(
        matches!(fate, CoveredFate::Restore { .. }),
        "the folder's own membership is the log's stand-in: got {fate:?}"
    );
    let CoveredFate::Restore { folder_id, stone } = fate else {
        unreachable!()
    };
    assert_eq!(folder_id, "f1");
    assert!(stone.moved, "the book left; it was not removed");
    assert_eq!(
        stone.title.as_deref(),
        Some("Dune"),
        "named by the copy that carries the name"
    );
    assert_eq!(stone.shelf_id, None, "with no shelf to remember, the folder's rung answers");

    let id = restore_covered_file(state, &file, &folder_id, &stone);

    let rows = state.library.books.get_untracked();
    assert_eq!(rows.len(), 2, "the link is back beside the copy, and not instead of it");
    let back = rows
        .iter()
        .find(|r| r.id() == id)
        .and_then(|r| r.book())
        .expect("a book row");
    assert!(matches!(back.origin, Origin::Linked { .. }));
    assert_eq!(back.path(), "/books/scifi/dune.md", "reading the folder's file again");
    assert_eq!(back.title.as_deref(), Some("Dune"), "wearing the name the shelf showed");
    assert!(!back.independent, "it is the folder's book, not a private one");
    let copy = rows
        .iter()
        .find(|r| r.id() == "b1")
        .and_then(|r| r.book())
        .expect("the copy");
    assert!(copy.origin.is_stored(), "and the copy the reader moved out is untouched");
    let shelves = state.library.shelves.get_untracked();
    let on = |sid: &str| {
        shelves
            .iter()
            .find(|s| s.id == sid)
            .map(|s| s.books.clone())
            .unwrap_or_default()
    };
    assert_eq!(
        on("sub"),
        vec![id],
        "on the rung the folder names for the file — not the one the copy is on"
    );
    assert_eq!(
        on("mid"),
        vec!["b1".to_string()],
        "and the copy stays where the reader put it"
    );
    assert!(on("fs").is_empty());
}

/// The same shape arriving through a folder walk rather than a loose file:
/// the copy holds the fingerprint the walk measured, so the walk's own
/// one-row-per-fingerprint rule would answer with the COPY's id and file the
/// copy on the rung. What the reader asked for is the file's linked book.
#[test]
fn a_walk_mints_the_link_beside_the_copy_that_holds_its_fingerprint() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let file = found_under("/books", "/books/dune.md", 7);
    let mut copy = stored("b1", "/books/dune.md", "/store/b1.md", 7);
    copy.as_book_mut().expect("a book").title = Some("Dune".to_string());
    state.library.books.set(vec![copy]);
    let mut one = folder("f1", "/books", &[7], Vec::new());
    one.shelf_map.insert(String::new(), "fs".to_string());
    state.library.folders.set(vec![one]);
    state.library.shelves.set(vec![plain("fs")]);

    let mut books = state.library.books.get_untracked();
    let mut folder = state.library.folders.get_untracked().remove(0);
    let empty_copies: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let empty_measured: std::collections::HashMap<String, Fingerprint> = std::collections::HashMap::new();
    let empty_copy_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    let planned_name: Option<String> = None;
    let landing = Landing {
        copies: &empty_copies,
        copy_measured: &empty_measured,
        copy_paths: &empty_copy_paths,
        planned_name: &planned_name,
        root: "/books",
        in_place: true,
        merged: false,
        now: 1,
    };
    let mut new_shelves = Vec::new();
    let minted = mint_walked_row(
        &mut books,
        &mut folder,
        &landing,
        "b2".to_string(),
        &file,
        &mut new_shelves,
    );
    let Minted::Placed { id, shelf } = minted else {
        panic!("the file owes a row of its own");
    };
    assert_eq!(id, "b2", "the link is its own row, not the copy's id");
    assert_eq!(shelf, "fs", "filed on the folder's own rung");
    assert_eq!(books.len(), 2, "and the copy is still standing");
    let back = books
        .iter()
        .find(|r| r.id() == "b2")
        .and_then(|r| r.book())
        .expect("a book row");
    assert!(matches!(back.origin, Origin::Linked { .. }));
    assert_eq!(back.path(), "/books/dune.md");
    assert!(!back.independent, "the folder's book is a shared row");
    assert!(folder.placed.contains(&file.fp), "and the folder still answers for the file");
}

/// The same walk over a COPYING folder keeps the rule whole: a second copy
/// of a file the library already copied is the duplicate the one-row rule
/// exists to prevent, so the arrival resolves to the copy that is there.
#[test]
fn a_copying_folder_never_mints_a_second_copy_of_its_own_file() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let file = found_under("/books", "/books/dune.md", 7);
    state.library.books.set(vec![stored("b1", "/books/dune.md", "/store/b1.md", 7)]);
    let mut one = folder("f1", "/books", &[7], Vec::new());
    one.opts.in_place = false;
    one.shelf_map.insert(String::new(), "fs".to_string());
    state.library.folders.set(vec![one]);
    state.library.shelves.set(vec![plain("fs")]);

    let mut books = state.library.books.get_untracked();
    let mut folder = state.library.folders.get_untracked().remove(0);
    let mut copies: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    copies.insert("b2".to_string(), "/store/b2.md".to_string());
    let empty_measured: std::collections::HashMap<String, Fingerprint> = std::collections::HashMap::new();
    let empty_copies: std::collections::HashSet<String> = std::collections::HashSet::new();
    let planned_name: Option<String> = None;
    let landing = Landing {
        copies: &copies,
        copy_measured: &empty_measured,
        copy_paths: &empty_copies,
        planned_name: &planned_name,
        root: "/books",
        in_place: false,
        merged: false,
        now: 1,
    };
    let mut new_shelves = Vec::new();
    let minted = mint_walked_row(
        &mut books,
        &mut folder,
        &landing,
        "b2".to_string(),
        &file,
        &mut new_shelves,
    );
    let Minted::Placed { id, .. } = minted else {
        panic!("the copy folder owes a placement");
    };
    assert_eq!(id, "b1", "the copy that is there is the row the import names");
    assert_eq!(books.len(), 1, "and no second copy of one file is made");
}

// -------------------------------------------------------------------
// A member of the tree standing outside it.
// -------------------------------------------------------------------

/// A shelf cut from a watched folder, hanging on `parent`.
fn rung(
    id: &str,
    name: &str,
    folder_id: &str,
    rel: Option<&str>,
    parent: Option<&str>,
) -> Shelf {
    Shelf {
        id: id.to_string(),
        name: name.to_string(),
        kind: library_core::shelf::ShelfKind::Folder {
            folder_id: folder_id.to_string(),
            rel: rel.map(str::to_string),
        },
        books: Vec::new(),
        parent: parent.map(str::to_string),
        manual_parent: false,
    }
}

/// The reported shape: `Root/ > Mid/ > Deep/` imported as one tree, the
/// `Deep` rung removed, `Deep/` then imported on its own so it stands at the
/// top level. A re-import of `Root/` finds every book already standing.
///
/// The owner comes back with the state and is held beside it: the signals
/// are the owner's, so a test that drops it and then reads one is a test
/// that panics on a disposed value rather than on its own assertion.
fn displaced_state() -> (AppState, Owner) {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let mut outer = folder("f1", "/root", &[7], Vec::new());
    outer.shelf_map.insert(String::new(), "s1".to_string());
    outer.shelf_map.insert("mid".to_string(), "s2".to_string());
    let mut inner = folder("f2", "/root/mid/deep", &[7], Vec::new());
    inner.shelf_map.insert(String::new(), "s3".to_string());
    state.library.folders.set(vec![outer, inner]);
    state.library.shelves.set(vec![
        rung("s1", "root", "f1", None, None),
        rung("s2", "mid", "f1", Some("mid"), Some("s1")),
        rung("s3", "deep", "f2", None, None),
    ]);
    state.library.books.set(vec![linked("b1", "/root/mid/deep/dune.md", 7)]);
    state.library.shelves.update(|shelves| {
        if let Some(shelf) = shelves.iter_mut().find(|s| s.id == "s3") {
            shelf.books.push("b1".to_string());
        }
    });
    (state, owner)
}

#[test]
fn a_subfolder_imported_on_its_own_is_a_member_standing_outside_the_tree() {
    let (state, _owner) = displaced_state();
    let outer = state.library.folder("f1").expect("the tree");
    let walk = vec![
        found_under("/root", "/root/notes.md", 8),
        found_under("/root", "/root/mid/deep/dune.md", 7),
    ];

    let member = displaced_member(state, &outer, &walk).expect("a member outside the tree");
    assert_eq!(member.folder_id, "f2", "the folder that reads the subfolder on its own");
    assert_eq!(member.rel, "mid/deep", "on the rung its directory names in the tree");
    assert_eq!(member.shelf_id, "s3", "and the shelf the note names is that folder's own");
    assert_eq!(member.shelf_name, "deep");
}

#[test]
fn a_member_inside_the_tree_is_not_displaced() {
    let (state, _owner) = displaced_state();
    // The reader carried the shelf in by hand: it hangs under the tree's
    // own rung, so it is where the reader put it and no run asks about it.
    state.library.shelves.update(|shelves| {
        if let Some(shelf) = shelves.iter_mut().find(|s| s.id == "s3") {
            shelf.parent = Some("s2".to_string());
            shelf.manual_parent = true;
        }
    });
    let outer = state.library.folder("f1").expect("the tree");
    let walk = vec![found_under("/root", "/root/mid/deep/dune.md", 7)];
    assert!(
        displaced_member(state, &outer, &walk).is_none(),
        "a shelf inside the tree is not standing outside it"
    );
}

#[test]
fn a_member_the_walk_found_nothing_under_is_not_the_question() {
    let (state, _owner) = displaced_state();
    let outer = state.library.folder("f1").expect("the tree");
    // A walk of the tree's own root shelf only: the member's ground was not
    // part of what this import found, so it is not this import's question.
    let walk = vec![found_under("/root", "/root/notes.md", 8)];
    assert!(displaced_member(state, &outer, &walk).is_none());
}

#[test]
fn a_copying_subfolder_is_not_a_member_of_the_tree() {
    let (state, _owner) = displaced_state();
    // Its copies are the library's own books rather than the tree's rung,
    // so a stored shelf standing at the top level is none of this run's
    // business — the same rule the gate keeps.
    state.library.folders.update(|folders| {
        if let Some(inner) = folders.iter_mut().find(|f| f.id == "f2") {
            inner.opts.in_place = false;
        }
    });
    let outer = state.library.folder("f1").expect("the tree");
    let walk = vec![found_under("/root", "/root/mid/deep/dune.md", 7)];
    assert!(displaced_member(state, &outer, &walk).is_none());
}

#[test]
fn the_shallowest_member_answers_because_the_deeper_one_comes_with_it() {
    let (state, _owner) = displaced_state();
    let mut deeper = folder("f3", "/root/mid", &[9], Vec::new());
    deeper.shelf_map.insert(String::new(), "s4".to_string());
    state.library.folders.update(|folders| folders.push(deeper));
    state.library.shelves.update(|shelves| {
        shelves.push(rung("s4", "mid", "f3", None, None));
    });
    let outer = state.library.folder("f1").expect("the tree");
    let walk = vec![
        found_under("/root", "/root/mid/dune.md", 9),
        found_under("/root", "/root/mid/deep/dune.md", 7),
    ];
    let member = displaced_member(state, &outer, &walk).expect("a member");
    assert_eq!(member.folder_id, "f3", "the rung nearest the root answers");
    assert_eq!(member.rel, "mid");
}

#[test]
fn putting_a_member_back_gives_the_tree_its_rung_and_retires_the_folder() {
    let (state, _owner) = displaced_state();
    // The nested folder's own subfolder, which comes back as a rung under
    // the returning one, and a removal it holds for a book the reader
    // deleted there — which must not be resurrected by the tree's next scan.
    state.library.shelves.update(|shelves| {
        shelves.push(rung("s5", "deeper", "f2", Some("deeper"), Some("s3")));
    });
    state.library.folders.update(|folders| {
        if let Some(inner) = folders.iter_mut().find(|f| f.id == "f2") {
            inner.shelf_map.insert("deeper".to_string(), "s5".to_string());
            inner.placed.insert(fp(9));
            inner.ignored.push(stone(9, "/root/mid/deep/deeper/gone.md", false, Some("s5")));
        }
    });

    reclaim_rung(state, "f1", "f2", "mid/deep", "s3");

    let shelves = state.library.shelves.get_untracked();
    let back = shelves.iter().find(|s| s.id == "s3").expect("the returning shelf");
    assert_eq!(back.parent.as_deref(), Some("s2"), "hung on the rung its directory names");
    assert!(!back.manual_parent, "and the disk owns that place again");
    assert_eq!(
        back.kind,
        library_core::shelf::ShelfKind::Folder {
            folder_id: "f1".to_string(),
            rel: Some("mid/deep".to_string())
        },
        "owned by the tree, on the tree's own key"
    );
    assert_eq!(
        back.books,
        vec!["b1".to_string()],
        "its books came with it — a shelf is a list of ids and the ids did not move"
    );
    let deeper = shelves.iter().find(|s| s.id == "s5").expect("its own subfolder");
    assert_eq!(
        deeper.kind,
        library_core::shelf::ShelfKind::Folder {
            folder_id: "f1".to_string(),
            rel: Some("mid/deep/deeper".to_string())
        },
        "and the shelf below it took the key below the returning one"
    );
    assert_eq!(deeper.parent.as_deref(), Some("s3"), "still hanging under it");

    let folders = state.library.folders.get_untracked();
    assert_eq!(folders.len(), 1, "one ground, one folder");
    let tree = &folders[0];
    assert_eq!(tree.id, "f1");
    assert_eq!(tree.shelf_map.get("mid/deep").map(String::as_str), Some("s3"));
    assert_eq!(tree.shelf_map.get("mid/deep/deeper").map(String::as_str), Some("s5"));
    assert!(
        tree.placed.contains(&fp(7)) && tree.placed.contains(&fp(9)),
        "the ledger followed the ground, so the tree's next scan adds nothing back"
    );
    assert!(
        tree.ignored.iter().any(|entry| entry.fp == fp(9) && !entry.moved),
        "and the removal the nested folder held is the tree's now"
    );
}

#[test]
fn putting_a_member_back_mints_the_rungs_the_tree_lost() {
    let (state, _owner) = displaced_state();
    // The reader removed the rung ABOVE the member too, so the tree has no
    // shelf for "mid": a returning shelf hung on nothing renders nowhere.
    state.library.shelves.update(|shelves| shelves.retain(|s| s.id != "s2"));
    state.library.folders.update(|folders| {
        if let Some(outer) = folders.iter_mut().find(|f| f.id == "f1") {
            outer.shelf_map.remove("mid");
        }
    });

    reclaim_rung(state, "f1", "f2", "mid/deep", "s3");

    let shelves = state.library.shelves.get_untracked();
    let folders = state.library.folders.get_untracked();
    let tree = &folders[0];
    let mid = tree.shelf_map.get("mid").expect("the rung above was minted");
    let mid = shelves.iter().find(|s| &s.id == mid).expect("and stands");
    assert_eq!(mid.name, "mid", "named by its directory");
    assert_eq!(mid.parent.as_deref(), Some("s1"), "hanging on the tree's root shelf");
    let back = shelves.iter().find(|s| s.id == "s3").expect("the returning shelf");
    assert_eq!(back.parent.as_deref(), Some(mid.id.as_str()), "with the member under it");
}

#[test]
fn an_answer_about_a_shelf_that_went_does_nothing_at_all() {
    let (state, _owner) = displaced_state();
    // The note can outlive the shelf it names: a reader who removes it
    // while the modal is up gets no move, and no folder folded away for
    // nothing.
    state.library.shelves.update(|shelves| shelves.retain(|s| s.id != "s3"));

    reclaim_rung(state, "f1", "f2", "mid/deep", "s3");

    let folders = state.library.folders.get_untracked();
    assert_eq!(folders.len(), 2, "the nested folder is still the nested folder");
    assert!(folders.iter().any(|f| f.id == "f2"));
    let tree = folders.iter().find(|f| f.id == "f1").expect("the tree");
    assert_eq!(
        tree.shelf_map.get("mid/deep"),
        None,
        "and nothing was folded into it"
    );
}

/// The fold is the note's old answer, run instead of offered: the member
/// standing outside the tree goes back on the rung its directory names,
/// the folder reading it folds into the tree's ledger, and the run's
/// report names the shelf that went home — the light the note's close
/// rides lands on the shelf in its new place rather than on the rung the
/// tree lost.
#[test]
fn the_fold_puts_the_member_back_and_names_the_shelf_it_seated() {
    let (state, _owner) = displaced_state();
    let outer = state.library.folder("f1").expect("the tree");
    let walk = vec![found_under("/root", "/root/mid/deep/dune.md", 7)];
    let plan = RootPlan::default();

    let folded = run_fold(state, &plan, &outer, None, &walk)
        .expect("a member outside the tree is a fold the run owes");
    assert_eq!(folded.0, "s3", "the report names the member's shelf");
    assert_eq!(folded.1, "deep", "speaking the name it wore");

    let shelves = state.library.shelves.get_untracked();
    let back = shelves.iter().find(|s| s.id == "s3").expect("the shelf");
    assert_eq!(
        back.parent.as_deref(),
        Some("s2"),
        "back on the rung its directory names"
    );
    assert!(
        !back.manual_parent,
        "and the disk owns that place again"
    );
    assert_eq!(
        state.library.folders.get_untracked().len(),
        1,
        "one ground, one folder"
    );

    // A second run finds no member outside the tree: the fold already
    // happened, and a run that folds nothing reports nothing.
    let again = run_fold(state, &plan, &outer, None, &walk);
    assert!(again.is_none(), "the member is inside the tree now");
}

/// The plan's own fold outruns the search for members: a run that walked
/// the PICKED folder's ledger seats the shelf that run minted, on the
/// rung the plan named, whatever else stands where.
#[test]
fn the_planned_fold_seats_the_run_s_own_shelf() {
    let (state, _owner) = displaced_state();
    let inner = state.library.folder("f2").expect("the picked folder");
    let plan = RootPlan {
        fold: Some(("f1".to_string(), "mid/deep".to_string())),
        ..Default::default()
    };
    let folded = run_fold(state, &plan, &inner, Some("s3"), &[])
        .expect("the plan names the rung and the run names the shelf");
    assert_eq!(folded.0, "s3");
    let shelves = state.library.shelves.get_untracked();
    let back = shelves.iter().find(|s| s.id == "s3").expect("the shelf");
    assert_eq!(back.parent.as_deref(), Some("s2"));
    assert_eq!(state.library.folders.get_untracked().len(), 1);
}

// -------------------------------------------------------------------
// The read-at-place gate and the copies a run owes.
// -------------------------------------------------------------------

fn linked(id: &str, path: &str, n: u32) -> Row {
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

fn stored(id: &str, src: &str, store: &str, n: u32) -> Row {
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

#[test]
fn the_gate_answers_by_the_rung_the_pick_names() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let mut one = folder("f1", "/books", &[7], Vec::new());
    one.shelf_map.insert(String::new(), "fs".to_string());
    one.shelf_map.insert("scifi".to_string(), "sub".to_string());
    state.library.folders.set(vec![one]);
    state.library.shelves.set(vec![plain("fs"), plain("sub")]);

    // The tree's own root: the continuation's shape — a walk on the tree's
    // own ledger, the root shelf as the light, and the note only if the walk
    // finds nothing.
    let covered = covered_shelf(state, "/books").expect("covered");
    assert_eq!(covered.shelf_id, "fs");
    assert_eq!(covered.shelf_name, "fs");
    assert_eq!(covered.tree_root, "/books");

    // A rung inside the tree: the SAME shape — the walk runs on the covering
    // tree, so a book a removal logged inside the rung comes back on a
    // re-pick of the rung — and the light is the rung's own shelf, because
    // the rung is the ground the reader asked about.
    let covered = covered_shelf(state, "/books/scifi").expect("covered");
    assert_eq!(covered.shelf_id, "sub");
    assert_eq!(
        covered.tree_root, "/books",
        "the reconciliation is the covering tree's, not a second door on the rung"
    );

    // Ground no in-place tree holds is no gate at all.
    assert!(covered_shelf(state, "/other").is_none());
    // A copying folder's tree is not the gate's business: its copies are
    // the library's to make another of.
    let mut copying = folder("f2", "/comics", &[], Vec::new());
    copying.opts.in_place = false;
    copying.shelf_map.insert(String::new(), "cs".to_string());
    state.library.folders.update(|folders| folders.push(copying));
    state.library.shelves.update(|shelves| shelves.push(plain("cs")));
    assert!(covered_shelf(state, "/comics").is_none());
    // A rung whose shelf has died is no standing shelf: the walk may
    // mint it again, so the gate stands aside.
    state
        .library
        .shelves
        .update(|shelves| shelves.retain(|each| each.id != "sub"));
    assert!(covered_shelf(state, "/books/scifi").is_none());
}

#[test]
fn a_folder_own_tree_answers_for_it_before_a_tree_it_stands_inside() {
    // Two in-place trees, one inside the other: the inner folder's own
    // root shelf is the gate's answer for its root, not the outer tree's
    // rung for it — whichever order the folders are stored in.
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let mut inner = folder("f1", "/books/scifi", &[7], Vec::new());
    inner.shelf_map.insert(String::new(), "sub".to_string());
    let mut outer = folder("f2", "/books", &[8], Vec::new());
    outer.shelf_map.insert(String::new(), "fs".to_string());
    outer.shelf_map.insert("scifi".to_string(), "outersub".to_string());
    state.library.folders.set(vec![outer, inner]);
    state
        .library
        .shelves
        .set(vec![plain("fs"), plain("outersub"), plain("sub")]);

    let covered = covered_shelf(state, "/books/scifi").expect("covered");
    assert_eq!(
        covered.tree_root, "/books/scifi",
        "the folder's own tree answers for it"
    );
    assert_eq!(covered.shelf_id, "sub", "and its own root shelf is the light");
}

#[test]
fn a_replace_takes_the_linked_books_and_leaves_the_copies() {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    // The folder's two linked books, and a stored copy that came home
    // onto one of its shelves: the replace is about the instances that
    // read the OS folder, and the copy is exactly what the shelf ends up
    // holding, so it stands.
    state.library.books.set(vec![
        linked("b1", "/books/a.md", 1),
        linked("b2", "/books/b.md", 2),
        stored("b3", "/books/c.md", "/store/b3.md", 3),
    ]);
    let mut shelf = plain("fs");
    shelf.books = vec!["b1".to_string(), "b2".to_string(), "b3".to_string()];
    state.library.shelves.set(vec![shelf]);
    let mut one = folder("f1", "/books", &[1, 2], Vec::new());
    one.shelf_map.insert(String::new(), "fs".to_string());
    state.library.folders.set(vec![one]);

    let mut doomed = replace_rows_of_tree(state, "/books");
    doomed.sort();
    assert_eq!(
        doomed,
        vec!["b1".to_string(), "b2".to_string()],
        "the linked books the folder's ledger answers for, and nothing else"
    );

    purge_folder_linked_books(state, "/books");

    let rows = state.library.books.get_untracked();
    assert_eq!(rows.len(), 1, "the copy that came home is what stands");
    assert_eq!(rows[0].id(), "b3");
    let shelves = state.library.shelves.get_untracked();
    assert_eq!(
        shelves[0].books,
        vec!["b3".to_string()],
        "the linked books came off the shelf they were filed on"
    );
    let folders = state.library.folders.get_untracked();
    assert_eq!(
        folders[0].ignored.len(),
        2,
        "and the folder's ledger remembers them — the logs the copy import spends as it lands"
    );
    assert!(folders[0].ignored.iter().all(|entry| !entry.moved));
    assert!(
        folders[0].placed.contains(&fp(1)) && folders[0].placed.contains(&fp(2)),
        "the placements stay: they are what keeps a rescan quiet until the copies land"
    );
}

// -------------------------------------------------------------------
// The reconciliation's merge.
// -------------------------------------------------------------------

/// A watched tree with two rungs and three books, one per shape a re-import
/// meets: a book still on its rung, a book the reader filed onto a shelf of
/// their own, and a book whose rung holds nothing.
fn reconciled_state() -> (AppState, Owner) {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    let mut one = folder("f1", "/books", &[1, 2, 3], Vec::new());
    one.shelf_map.insert(String::new(), "s1".to_string());
    one.shelf_map.insert("scifi".to_string(), "s2".to_string());
    state.library.folders.set(vec![one]);
    let mut root = library_core::testkit::folder_shelf("s1", "Books", "f1", None, &[], None);
    root.books = vec!["b1".to_string()];
    let rung =
        library_core::testkit::folder_shelf("s2", "scifi", "f1", Some("scifi"), &[], Some("s1"));
    let mut mine = plain("mine");
    mine.books = vec!["b2".to_string()];
    state.library.shelves.set(vec![root, rung, mine]);
    state.library.books.set(vec![
        linked("b1", "/books/a.md", 1),
        linked("b2", "/books/scifi/b.md", 2),
        linked("b3", "/books/scifi/c.md", 3),
    ]);
    (state, owner)
}

/// The walk of `/books`, as the shell's scan would report it.
fn walked() -> Vec<FoundFile> {
    vec![
        found_under("/books", "/books/a.md", 1),
        found_under("/books", "/books/scifi/b.md", 2),
        found_under("/books", "/books/scifi/c.md", 3),
    ]
}

/// The merge stage's answer over that walk, with no file owed a row of its own.
fn returned(state: AppState, folder_id: &str) -> Vec<String> {
    let rows = state.library.books.get_untracked();
    let found = walked();
    let copy_paths: HashSet<String> = HashSet::new();
    let registry = library_core::ledger::registry_of(&rows);
    let snap = Snapshot {
        books: &rows,
        registry: &registry,
        found: &found,
        copy_paths: &copy_paths,
    };
    returned_memberships(state, &snap, folder_id, &[])
        .into_iter()
        .map(|(row_id, _)| row_id)
        .collect()
}

#[test]
fn a_re_import_re_files_the_books_its_tree_stopped_holding() {
    // The merge half of a re-pick, and the half the ledger's table cannot
    // answer: a book the reader filed elsewhere and a book whose shelf holds
    // nothing are both a Skip — the content is known and its address has not
    // moved — and both are a book the reader picking this folder again is asking
    // to see on it. Without this the walk reported nothing new, lit the folder,
    // and left the books it was asked for off the shelf it lit.
    let (state, _owner) = reconciled_state();
    assert_eq!(
        returned(state, "f1"),
        vec!["b2".to_string(), "b3".to_string()],
        "the book on a shelf of the reader's, and the book on no shelf at all"
    );
}

#[test]
fn a_tree_that_holds_its_books_owes_no_merge() {
    // The other half of the same rule: a re-pick of a tree whose shelves hold
    // everything the walk found is the note's own case, and a merge that
    // re-filed books already filed would turn every re-import into an import.
    let (state, _owner) = reconciled_state();
    state.library.shelves.update(|shelves| {
        for shelf in shelves.iter_mut() {
            if shelf.id == "s2" {
                shelf.books = vec!["b2".to_string(), "b3".to_string()];
            }
        }
    });
    assert!(
        returned(state, "f1").is_empty(),
        "every book the walk found is on a shelf the folder owns"
    );
}

#[test]
fn a_merge_is_about_one_tree_and_not_the_shelf_beside_it() {
    // The question is asked of the folder that is walking: another folder's tree
    // holding the same books is not this tree holding them, and a reader's own
    // shelf is not a rung of it either.
    let (state, _owner) = reconciled_state();
    let mut other = folder("f2", "/elsewhere", &[], Vec::new());
    other.shelf_map.insert(String::new(), "theirs".to_string());
    state.library.folders.update(|folders| folders.push(other));
    state.library.shelves.update(|shelves| {
        shelves.push(library_core::testkit::folder_shelf(
            "theirs",
            "Elsewhere",
            "f2",
            None,
            &["b1", "b2", "b3"],
            None,
        ));
    });
    assert_eq!(
        returned(state, "f1"),
        vec!["b2".to_string(), "b3".to_string()],
        "another tree's shelves do not answer for this one's"
    );
}
