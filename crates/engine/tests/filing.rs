//! The filing run's own tests: what it asks for, files, and lets go.

use mareader_engine::library::{Bound, Proposal, Spot, State, apply, undo};
use mareader_library::{Book, BookId, Origin};

/// `copies` imported books, then one read in place.
fn library(copies: usize) -> State {
    let mut state = State::default();
    state.seed_copies(copies);
    state.seed(1);
    state
}

/// The imported books, in a steady order.
fn ids(state: &State) -> Vec<BookId> {
    let model = &state.model;
    let stored = |b: &&Book| b.home().is_some_and(|l| l.origin == Origin::Store);
    let mut ids: Vec<BookId> = model.books().filter(stored).map(|b| b.id).collect();
    ids.sort();
    ids
}

#[test]
fn only_present_imported_books_on_no_shelf_are_asked_about() {
    let mut state = State::default();
    state.seed(3);
    let none = state.ask_organize(None);
    assert!(none.is_err(), "books read in place are never organized");
    let mut state = library(6);
    let all = ids(&state);
    assert_eq!(all.len(), 6);
    let shelf = state.model.create_shelf("Kept", None, 1).ok();
    let first = all.first().copied();
    if let (Some(book), Some(shelf)) = (first, shelf) {
        state.model.file(book, shelf, 1);
    }
    // One file is gone from the store.
    state
        .model
        .check_present(|p| !p.to_string_lossy().starts_with("/store/2-"));
    let ask = state
        .ask_organize(None)
        .ok()
        .unwrap_or_else(|| panic!("asked"));
    // Six, less the shelved one and the missing one.
    assert_eq!(ask.books.len(), 4);
    assert_eq!(ask.shelves.len(), 1);
    assert!(ask.books.iter().all(|b| Some(b.id) != first));
    let picked = state.ask_organize(Some(first.as_slice()));
    assert!(picked.is_err(), "a shelved book alone has nothing to do");
}

#[test]
fn an_imported_book_once_opened_can_still_be_organized() {
    let mut state = library(1);
    let first = ids(&state).first().copied();
    let book = first.and_then(|id| state.model.book(id));
    let opened = book.and_then(|b| {
        Some(mareader_library::Opened {
            path: b.path()?.to_path_buf(),
            measure: mareader_library::Measure {
                content: b.content,
                stamp: Default::default(),
            },
            format: b.format,
            title: b.title.value.clone(),
            author: None,
        })
    });
    let opened = opened.unwrap_or_else(|| panic!("a copy with its file"));
    state.model.record_open(opened, 9);
    assert_eq!(ids(&state), first.into_iter().collect::<Vec<_>>());
    let ask = state.ask_organize(None);
    assert_eq!(ask.map(|a| a.books.len()).ok(), Some(1));
}

#[test]
fn a_plan_files_books_on_new_and_standing_shelves_then_undoes() {
    let mut state = library(3);
    let all = ids(&state);
    let (a, b) = (all.first().copied(), all.get(1).copied());
    let (Some(a), Some(b)) = (a, b) else {
        panic!("two books")
    };
    let Ok(old) = state.model.create_shelf("Fiction", None, 1) else {
        panic!("shelf")
    };
    let plan = Proposal {
        shelves: vec![
            ("Classics".into(), Some(Spot::Shelf(old))),
            ("Victorian".into(), Some(Spot::New(0))),
            ("Never used".into(), None),
        ],
        placed: vec![(a, Spot::New(1)), (b, Spot::Shelf(old))],
        left: Vec::new(),
    };
    let done = apply(&mut state.model, &plan, 2);
    assert_eq!(done.filed.len(), 2);
    assert_eq!(done.skipped, 0);
    // Classics holds only Victorian; the unused shelf went again.
    assert_eq!(done.made.len(), 2);
    let names: Vec<String> = (state.model.shelves())
        .map(|s| s.name.value.clone())
        .collect();
    assert!(!names.contains(&"Never used".to_owned()), "{names:?}");
    let on_a = state.model.shelves_of(a);
    let victorian = done.made.last().copied();
    assert_eq!(on_a.into_iter().next(), victorian);
    undo(&mut state.model, &done, 3);
    assert!(state.model.shelves_of(a).is_empty());
    assert!(state.model.shelves_of(b).is_empty());
    // Only the standing shelf is left.
    assert_eq!(state.model.shelves().count(), 1);
}

#[test]
fn a_plan_names_each_book_s_shelf_for_its_card() {
    let mut state = library(3);
    let all = ids(&state);
    let [a, b, c] = all.as_slice() else {
        panic!("three books")
    };
    let Ok(old) = state.model.create_shelf("Fiction", None, 1) else {
        panic!("shelf")
    };
    let plan = Proposal {
        shelves: vec![("Poems".into(), None)],
        placed: vec![(*a, Spot::New(0)), (*b, Spot::Shelf(old))],
        left: vec![*c],
    };
    let shelf = |name: &str| Some(Bound::Shelf(name.into()));
    assert_eq!(plan.bound(&state.model, *a), shelf("Poems"));
    assert_eq!(plan.bound(&state.model, *b), shelf("Fiction"));
    assert_eq!(plan.bound(&state.model, *c), Some(Bound::Stays));
    let other = state.model.books().map(|x| x.id).find(|x| !all.contains(x));
    let other = other.unwrap_or_else(|| panic!("the book read in place"));
    assert_eq!(plan.bound(&state.model, other), None);
}

#[test]
fn books_that_changed_meanwhile_stay_put() {
    let mut state = library(3);
    let all = ids(&state);
    let (Some(a), Some(b)) = (all.first().copied(), all.get(1).copied()) else {
        panic!("two books")
    };
    let Ok(mine) = state.model.create_shelf("Mine", None, 1) else {
        panic!("shelf")
    };
    // The reader shelved one while the model thought.
    state.model.file(a, mine, 1);
    let plan = Proposal {
        shelves: vec![("Poems".into(), None), ("Lonely".into(), None)],
        placed: vec![(a, Spot::New(1)), (b, Spot::New(0))],
        left: Vec::new(),
    };
    let done = apply(&mut state.model, &plan, 2);
    assert_eq!(done.filed.len(), 1);
    assert_eq!(done.skipped, 1);
    assert_eq!(done.made.len(), 1, "the shelf it'd have had isn't made");
    assert_eq!(state.model.shelves_of(a).len(), 1);
}
