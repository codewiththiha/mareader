use super::*;
use library_core::book::{Book, Fingerprint, Origin, Row};
use library_core::conflict::Placement;
use library_core::folder::FolderOpts;
use library_core::shelf::Shelf;
use reader_core::format::Format;

fn fp(n: u32) -> Fingerprint {
    library_core::testkit::fp_n(n)
}

/// The cover queue skips anything that is not a PDF, so a host test that
/// lands a book never starts the wasm render chain.
fn row(id: &str, title: &str, path: &str, n: u32) -> Row {
    Row::Book(Book {
        title: Some(title.to_string()),
        fp: fp(n),
        format: Format::Markdown,
        origin: Origin::Linked {
            src: path.to_string(),
        },
        ..library_core::testkit::book(id)
    })
}

fn row_at(id: &str, title: &str, path: &str, n: u32, page: u32) -> Row {
    let mut row = row(id, title, path, n);
    row.as_book_mut().unwrap().page = page;
    row.as_book_mut().unwrap().num_pages = 400;
    row
}

fn shelf(id: &str, members: &[&str]) -> Shelf {
    library_core::testkit::plain_shelf(id, members)
}

fn stored_row(id: &str, title: &str, src_path: &str, store: &str, n: u32) -> Row {
    let mut book = Book::new(
        id.to_string(),
        fp(n),
        Format::Markdown,
        Origin::Stored {
            src: Some(src_path.to_string()),
            store: store.to_string(),
        },
        0,
    );
    book.title = Some(title.to_string());
    Row::Book(book)
}

fn folder_in_place(id: &str, root: &str, placed: &[u32]) -> library_core::folder::WatchedFolder {
    library_core::folder::WatchedFolder {
        placed: placed.iter().copied().map(fp).collect(),
        ..library_core::testkit::watched_folder(id, root)
    }
}

fn file(name: &str, n: u32) -> library_core::scan::FoundFile {
    library_core::scan::FoundFile {
        rel: format!("{name}.md"),
        path: format!("/incoming/{name}.md"),
        ext: "md".to_string(),
        size: u64::from(n),
        fp: fp(n),
    }
}

/// The pair a test has to hold, because a signal written under a dropped owner is a signal nobody can read.
fn library(rows: Vec<Row>, shelves: Vec<Shelf>) -> (Owner, AppState) {
    let owner = Owner::new();
    owner.set();
    let state = AppState::default();
    state.library.books.set(rows);
    state.library.shelves.set(shelves);
    (owner, state)
}

fn the_ask(state: AppState, arrival: Arrival) -> ConflictAsk {
    let (clean, asks) = screen(state, vec![arrival]);
    assert!(clean.is_empty(), "the level has no room for this name");
    asks.into_iter().next().expect("a question")
}

#[test]
fn a_level_with_room_for_the_name_lands_without_asking() {
    let (_owner, state) = library(
        vec![row("b1", "Dune", "/one/dune.md", 1)],
        vec![shelf("s", &["b1"]), shelf("t", &[])],
    );
    let (clean, asks) = screen(state, vec![Arrival::import(file("Report", 2), "t", None)]);
    assert_eq!(clean.len(), 1);
    assert!(asks.is_empty());
    assert!(!state.library.conflict.open.get_untracked());
}

#[test]
fn a_name_the_level_already_holds_asks_and_names_the_row() {
    let (_owner, state) = library(
        vec![row("b1", "Dune", "/one/dune.md", 1)],
        vec![shelf("s", &["b1"])],
    );
    let ask = the_ask(state, Arrival::import(file("dune", 2), "s", None));
    assert_eq!(ask.existing_id, "b1");
    assert_eq!(ask.existing_name, "Dune");
    assert_eq!(ask.arrival.name, "dune");
    assert!(ask.arrival.file.is_some());
}

#[test]
fn a_batch_lands_its_clean_half_and_queues_its_questions() {
    let (_owner, state) = library(
        vec![
            row("b1", "Dune", "/one/dune.md", 1),
            row("b2", "Foundation", "/one/foundation.md", 2),
        ],
        vec![shelf("s", &["b1", "b2"])],
    );
    let arrivals = vec![
        Arrival::import(file("neuromancer", 3), "s", None),
        Arrival::import(file("dune", 4), "s", None),
        Arrival::import(file("foundation", 5), "s", None),
    ];
    let (clean, asks) = screen(state, arrivals);
    assert_eq!(clean.len(), 1, "the name nobody holds lands now");
    assert_eq!(clean[0].name, "neuromancer");
    assert_eq!(asks.len(), 2, "and the two that collide ask");
    assert_eq!(asks[0].existing_id, "b1");
    assert_eq!(asks[1].existing_id, "b2");

    raise(state, asks);
    assert!(state.library.conflict.open.get_untracked());
    let on_screen = state
        .library
        .conflict
        .ask
        .get_untracked()
        .expect("a question");
    assert_eq!(on_screen.existing_id, "b1", "one question at a time");
    assert_eq!(
        state.library.conflict_waiting.get_untracked().len(),
        1,
        "and the rest wait rather than being dropped"
    );

    let (_clean, more) = screen(state, vec![Arrival::import(file("dune", 6), "s", None)]);
    raise(state, more);
    assert_eq!(
        state
            .library
            .conflict
            .ask
            .get_untracked()
            .map(|a| a.existing_id)
            .as_deref(),
        Some("b1"),
        "the question on screen is still the one that was asked first"
    );
    assert_eq!(state.library.conflict_waiting.get_untracked().len(), 2);

    cancel(state);
    assert!(state.library.conflict.ask.get_untracked().is_none());
    assert!(state.library.conflict_waiting.get_untracked().is_empty());
    assert!(!state.library.conflict.open.get_untracked());
}

#[test]
fn already_imported_places_nothing_and_lights_the_row_it_names() {
    let (_owner, state) = library(
        vec![row("b1", "Dune", "/one/dune.md", 1)],
        vec![shelf("s", &["b1"])],
    );
    let ask = the_ask(state, Arrival::import(file("dune", 2), "s", None));
    raise(state, vec![ask]);

    answer_placement(state, Placement::Open);

    assert_eq!(
        state.library.books.get_untracked().len(),
        1,
        "no row was written"
    );
    assert_eq!(state.library.shelf.get_untracked(), "s");
    let first = state.library.reveal.get_untracked().expect("a reveal");
    assert_eq!(first.id, "b1");
    assert!(!state.library.conflict.open.get_untracked());

    // The nonce is what makes a second reveal of the same row a second gesture rather than an equal value nobody is told about.
    let ask = the_ask(state, Arrival::import(file("dune", 2), "s", None));
    raise(state, vec![ask]);
    answer_placement(state, Placement::Open);
    let second = state
        .library
        .reveal
        .get_untracked()
        .expect("a second reveal");
    assert_ne!(first.nonce, second.nonce);
}

#[test]
fn make_link_puts_a_pointer_on_the_level_and_no_second_book() {
    let (_owner, state) = library(
        vec![row("b1", "Dune", "/one/dune.md", 1)],
        vec![shelf("s", &["b1"])],
    );
    let ask = the_ask(state, Arrival::import(file("dune", 1), "s", None));
    raise(state, vec![ask]);

    answer_placement(state, Placement::LinkOnly);

    let rows = state.library.books.get_untracked();
    assert_eq!(rows.len(), 2, "a link is a row, and not a second book");
    let link = rows.iter().find(|r| r.is_link()).expect("a link");
    assert_eq!(link.display_name(), "Dune", "it wears the name of its book");
    assert_eq!(link.target(), Some("b1"));
    assert_eq!(link.fp(), None, "and has no content identity at all");
    let shelves = state.library.shelves.get_untracked();
    let on_s = shelves
        .iter()
        .find(|s| s.id == "s")
        .map(|s| s.books.clone());
    assert_eq!(
        on_s,
        Some(vec!["b1".to_string(), link.id().to_string()]),
        "filed on the level it was dropped on, beside the book it points at"
    );
    assert!(!state.library.conflict.open.get_untracked());
}

#[test]
fn a_link_never_blocks_the_next_arrival_and_never_gets_opened() {
    let (_owner, state) = library(
        vec![
            row("b1", "Dune", "/one/dune.md", 1),
            Row::link("l1".into(), "Dune".into(), "b1".into(), 5),
        ],
        vec![shelf("s", &["l1"])],
    );
    let (clean, asks) = screen(state, vec![Arrival::import(file("dune", 2), "s", None)]);
    assert_eq!(clean.len(), 1);
    assert!(asks.is_empty());
    assert_eq!(state.library.row("l1").map(|r| r.is_link()), Some(true));
    assert_eq!(state.library.row_name("l1"), "Dune");
    assert_eq!(state.library.row_name("gone"), "");
}

#[test]
fn merge_keeps_the_row_that_was_here_and_folds_the_other_into_it() {
    // The row already here survives — its id is what every shelf and every storage key names.
    let (_owner, state) = library(
        vec![
            row_at("b1", "Dune", "/one/dune.md", 1, 12),
            row_at("b2", "Dune", "/two/dune.md", 2, 240),
        ],
        vec![shelf("s", &["b1"]), shelf("t", &["b2"]), shelf("u", &[])],
    );
    let ask = the_ask(state, Arrival::moved("b2", "Dune", "s", None));
    raise(state, vec![ask]);

    answer_placement(state, Placement::Merge);

    let rows = state.library.books.get_untracked();
    assert_eq!(rows.len(), 1, "two books became one");
    assert_eq!(
        rows[0].id(),
        "b1",
        "and the one that was here is the one that stayed"
    );
    assert_eq!(
        rows[0].book().map(|b| b.page),
        Some(240),
        "a merge never sends a reader backwards"
    );
    let shelves = state.library.shelves.get_untracked();
    let on = |id: &str| {
        shelves
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.books.clone())
            .unwrap_or_default()
    };
    assert_eq!(
        on("s"),
        vec!["b1".to_string()],
        "still on the level it was asked about"
    );
    assert_eq!(
        on("t"),
        vec!["b1".to_string()],
        "and on every shelf the dissolved row held"
    );
    assert!(!state.library.conflict.open.get_untracked());
}

#[test]
fn a_merge_after_a_move_leaves_the_level_the_book_departed() {
    // The regression this guards: a drag from "t" onto "s" answered with merge used to file the survivor onto "t" — the very shelf the move departed.
    let (_owner, state) = library(
        vec![
            row_at("b1", "Dune", "/one/dune.md", 1, 12),
            row_at("b2", "Dune", "/two/dune.md", 2, 240),
        ],
        vec![
            shelf("s", &["b1"]),
            shelf("t", &["b2"]),
            shelf("u", &["b2"]),
        ],
    );
    let ask = the_ask(state, Arrival::moved("b2", "Dune", "s", None).leaving("t"));
    raise(state, vec![ask]);

    answer_placement(state, Placement::Merge);

    let rows = state.library.books.get_untracked();
    assert_eq!(rows.len(), 1, "two books became one");
    let shelves = state.library.shelves.get_untracked();
    let on = |id: &str| {
        shelves
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.books.clone())
            .unwrap_or_default()
    };
    assert_eq!(
        on("s"),
        vec!["b1".to_string()],
        "the survivor keeps the level it was asked about"
    );
    assert_eq!(
        on("t"),
        Vec::<String>::new(),
        "and the level the move departed stays departed — that departure IS the move"
    );
    assert_eq!(
        on("u"),
        vec!["b1".to_string()],
        "every OTHER shelf the dissolved row held still joins the survivor"
    );
}

#[test]
fn the_covered_question_places_nothing_and_lights_the_folders_book() {
    let (_owner, state) = library(
        vec![row("b1", "Dune", "/books/dune.md", 1)],
        vec![shelf("fs", &["b1"]), shelf("s", &[])],
    );
    let ask = ConflictAsk::covered(
        Arrival::import(file("dune", 1), "s", None),
        "b1".to_string(),
        "Dune".to_string(),
        "f1".to_string(),
    );
    raise(state, vec![ask]);

    answer_covered(state, library_core::conflict::Placement::Open, false);

    assert_eq!(
        state.library.books.get_untracked().len(),
        1,
        "no row was written"
    );
    let id = state.library.reveal.get_untracked().expect("a reveal").id;
    assert_eq!(id, "b1", "and the light lands on the book the folder holds");
    assert!(!state.library.conflict.open.get_untracked());
}

#[test]
fn the_show_answer_imports_nothing_and_lights_the_shelf_that_is_here() {
    let (_owner, state) = library(Vec::new(), vec![shelf("s1", &[])]);
    state.library.shelf_conflict.ask.set(Some(ShelfConflictAsk {
        incoming_name: "Books".to_string(),
        existing_id: "s1".to_string(),
        existing_name: "Books".to_string(),
        root: "/books".to_string(),
        opts: FolderOpts {
            in_place: false,
            ..FolderOpts::default()
        },
        own: true,
    }));
    state.library.shelf_conflict.open.set(true);

    answer_shelf(state, library_core::conflict::Placement::Open);

    assert!(
        state.library.books.get_untracked().is_empty(),
        "no row was written"
    );
    let reveal = state.library.reveal.get_untracked().expect("a reveal");
    assert_eq!(
        reveal.id, "s1",
        "and the light lands on the shelf that is here"
    );
    assert!(
        !state.library.shelf_conflict.open.get_untracked(),
        "the answer closed the sheet"
    );
}

#[test]
fn replace_sends_the_row_that_was_here_out_and_seats_the_arrival_in_its_place() {
    let (_owner, state) = library(
        vec![
            row_at("b1", "Dune", "/one/dune.md", 1, 12),
            row_at("b2", "Dune", "/two/dune.md", 2, 240),
        ],
        vec![
            shelf("s", &["b1"]),
            shelf("t", &["b2"]),
            shelf("u", &["b1"]),
        ],
    );
    let ask = the_ask(state, Arrival::moved("b2", "Dune", "s", None));
    raise(state, vec![ask]);

    answer_placement(state, Placement::Replace);

    let rows = state.library.books.get_untracked();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id(), "b2", "the arrival is the book that is left");
    let shelves = state.library.shelves.get_untracked();
    let on = |id: &str| {
        shelves
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.books.clone())
            .unwrap_or_default()
    };
    assert_eq!(
        on("s"),
        vec!["b2".to_string()],
        "in the displaced row's slot"
    );
    assert_eq!(
        on("u"),
        vec!["b2".to_string()],
        "and on the other shelves it was filed on — a replace is not a quiet removal"
    );
    assert_eq!(
        on("t"),
        Vec::<String>::new(),
        "having left the one it was lifted from"
    );
}

#[test]
fn a_move_collision_is_the_other_question() {
    let (_owner, state) = library(
        vec![
            row_at("b1", "Dune", "/one/dune.md", 1, 12),
            row_at("b2", "Dune", "/two/dune.md", 2, 1),
        ],
        vec![shelf("s", &["b1"]), shelf("t", &["b2"])],
    );
    // The sheet offers merge, replace and as new — and never a link, which is an answer for an arrival that has no row of its own to keep.
    let (clean, asks) = screen(state, vec![Arrival::moved("b2", "Dune", "s", None)]);
    assert!(clean.is_empty());
    assert_eq!(asks.len(), 1);
    assert!(!asks[0].arrival.is_import());
    assert_eq!(asks[0].existing_id, "b1");
    let (clean, asks) = screen(state, vec![Arrival::import(file("dune", 3), "s", None)]);
    assert!(clean.is_empty());
    assert!(asks[0].arrival.is_import());
}

#[test]
fn add_as_new_renames_a_moved_row_and_then_moves_it() {
    let (_owner, state) = library(
        vec![
            row("b1", "Dune", "/one/dune.md", 1),
            row("b2", "Dune", "/two/dune.md", 2),
        ],
        vec![shelf("s", &["b1"]), shelf("t", &["b2"])],
    );
    let ask = the_ask(state, Arrival::moved("b2", "Dune", "s", None));
    raise(state, vec![ask]);

    answer_placement(state, Placement::KeepBoth);

    let rows = state.library.books.get_untracked();
    let renamed = rows
        .iter()
        .find(|r| r.id() == "b2")
        .map(Row::display_name)
        .unwrap_or_default();
    assert_eq!(renamed, "Dune_1", "the counter is the answer, not a dialog");
    let shelves = state.library.shelves.get_untracked();
    assert_eq!(
        shelves
            .iter()
            .find(|s| s.id == "s")
            .map(|s| s.books.clone()),
        Some(vec!["b1".to_string(), "b2".to_string()]),
        "and it landed on the shelf it was dropped on"
    );
    assert_eq!(
        shelves
            .iter()
            .find(|s| s.id == "t")
            .map(|s| s.books.clone()),
        Some(Vec::new()),
        "having left the one it was lifted from"
    );
    assert_eq!(
        state.library.row_name("b1"),
        "Dune",
        "the row that was already there keeps its own name"
    );
}

#[test]
fn a_link_answer_dissolves_the_dragged_row_into_a_pointer_and_binds_the_log() {
    // The pointer shape: a read-at-place book meets the library's own stored copy of its content. The folder's moved-out log binds to the survivor, so a later import of the file highlights the copy instead of minting a neighbour.
    let (_owner, state) = library(
        vec![
            row("b1", "Dune", "/books/dune.md", 1),
            stored_row("b2", "Dune", "/books/dune.md", "/store/b2.md", 2),
        ],
        vec![shelf("s", &["b1"]), shelf("t", &["b2"])],
    );
    state
        .library
        .folders
        .set(vec![folder_in_place("f1", "/books", &[1])]);
    let ask = the_ask(state, Arrival::moved("b1", "Dune", "t", None));
    raise(state, vec![ask]);

    answer_placement(state, Placement::LinkOnly);

    let rows = state.library.books.get_untracked();
    assert!(find_row(&rows, "b1").is_none(), "the dragged row is gone");
    let link = rows
        .iter()
        .find(|r| r.is_link())
        .expect("the level holds a pointer");
    assert_eq!(link.target(), Some("b2"), "pointing at the copy");
    let shelves = state.library.shelves.get_untracked();
    let on_t = shelves
        .iter()
        .find(|s| s.id == "t")
        .map(|s| s.books.clone());
    assert_eq!(
        on_t,
        Some(vec!["b2".to_string(), link.id().to_string()]),
        "the copy keeps its place and the pointer joins the level"
    );
    let folders = state.library.folders.get_untracked();
    let stone = folders[0]
        .ignored
        .iter()
        .find(|entry| entry.moved)
        .expect("a moved-out log");
    assert_eq!(stone.fp, fp(1), "for the file on disk");
    assert_eq!(
        stone.returned_row.as_deref(),
        Some("b2"),
        "bound to the row that represents it"
    );
}

#[test]
fn a_merge_into_the_stored_copy_binds_the_folder_log_to_the_survivor() {
    let (_owner, state) = library(
        vec![
            row("b1", "Dune", "/books/dune.md", 1),
            stored_row("b2", "Dune", "/books/dune.md", "/store/b2.md", 2),
        ],
        vec![shelf("s", &["b1"]), shelf("t", &["b2"])],
    );
    state
        .library
        .folders
        .set(vec![folder_in_place("f1", "/books", &[1])]);
    let ask = the_ask(state, Arrival::moved("b1", "Dune", "t", None));
    raise(state, vec![ask]);

    answer_placement(state, Placement::Merge);

    let rows = state.library.books.get_untracked();
    assert_eq!(rows.len(), 1, "two books became one");
    let folders = state.library.folders.get_untracked();
    let stone = folders[0]
        .ignored
        .iter()
        .find(|entry| entry.moved)
        .expect("a moved-out log");
    assert_eq!(stone.returned_row.as_deref(), Some("b2"));
}

#[test]
fn a_merge_of_two_different_books_writes_no_log() {
    // A log binding the folder to an unrelated survivor would send a re-import to the wrong row.
    let (_owner, state) = library(
        vec![
            row("b1", "Dune", "/books/dune.md", 1),
            row("b2", "Dune", "/other/dune.md", 2),
        ],
        vec![shelf("s", &["b1"]), shelf("t", &["b2"])],
    );
    state
        .library
        .folders
        .set(vec![folder_in_place("f1", "/books", &[1])]);
    let ask = the_ask(state, Arrival::moved("b1", "Dune", "t", None));
    raise(state, vec![ask]);

    answer_placement(state, Placement::Merge);

    let folders = state.library.folders.get_untracked();
    assert!(
        folders[0].ignored.is_empty(),
        "a different book's merge is no departure of THIS file"
    );
}
