//! The shelf's "Duplicate": a second instance of one thing, asked for by name.
//!
//! A duplicate is the app's own object from the moment it exists: nothing
//! about it is shared with what it came from. Not the bytes — a read-at-place
//! book, a stored copy and a link at a book all duplicate into the library's
//! own store, each in its own item folder — and not the reader's work: the
//! highlights land on the copy as its own list under its own id. A shelf
//! duplicates the same way: a second tree holding fresh copies of the books,
//! not a second door onto the same rows.
//!
//! The one thing that stays a pointer is a link at a shelf: a level holds
//! membership and never held a byte, so there is nothing to store.

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Book, Fingerprint, Origin, Row, find_row};
use library_core::conflict::{next_name, next_shelf_name};
use library_core::id;
use library_core::shelf::{self as shelves_ops, Shelf, ShelfKind};
use library_core::wire::BookFileRequest;

use crate::services as ipc;
use crate::services::covers;
use crate::services::import;
use crate::services::toast;
use runtime_contract::time::now_ms;

pub fn duplicate_row(state: crate::context::LibraryContext, row_id: &str) {
    duplicate_entries(state, std::slice::from_ref(&row_id.to_string()));
}

pub fn duplicate_shelf(state: crate::context::LibraryContext, shelf_id: &str) {
    duplicate_entries(state, std::slice::from_ref(&shelf_id.to_string()));
}

/// ENTRIES because a shelf's rows and the shelves themselves are both in
/// the list.
pub fn duplicate_entries(state: crate::context::LibraryContext, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let ids = ids.to_vec();
    spawn_local(async move {
        let mut landed: Vec<Duplicated> = Vec::new();
        for id in ids {
            if let Some(one) = duplicate_one(state, &id).await {
                landed.push(one);
            }
        }
        if landed.is_empty() {
            return;
        }
        covers::backfill_missing(state);
        crate::services::persist_library(state.library);
        toast(state, report(&landed));
    });
}

struct Duplicated {
    name: String,
    shelf: bool,
}

fn report(landed: &[Duplicated]) -> String {
    if landed.len() == 1 {
        return format!("Duplicated as “{}”.", landed[0].name);
    }
    let shelves = landed.iter().filter(|one| one.shelf).count();
    let books = landed.len() - shelves;
    let noun = match (books, shelves) {
        (_, 0) => "books",
        (0, _) => "shelves",
        _ => "shelves and books",
    };
    format!("Duplicated {} {noun}.", landed.len())
}

/// Every refusal but the missing book has already said so on the toast (the
/// missing book's menu row is disabled before the click can land). The two id
/// kinds are disjoint by prefix, so the row list answering "not mine" is the
/// shelf list's turn.
async fn duplicate_one(state: crate::context::LibraryContext, id: &str) -> Option<Duplicated> {
    let Some(row) = state.library.row(id) else {
        return duplicate_shelf_row(state, id)
            .await
            .map(|name| Duplicated { name, shelf: true });
    };
    match row {
        Row::Link { name, target, .. } => {
            let at = link_at(&state.library.books.get_untracked(), &target);
            match at {
                // A shelf's link is the one duplicate that stays a pointer: a level holds
                // no bytes, so there is nothing to store and nothing to separate.
                LinkAt::Shelf => Some(Duplicated {
                    name: duplicate_shelf_link(state, id, &name, &target),
                    shelf: false,
                }),
                // A book's link is a doorway onto the target's bytes: the duplicate is the
                // app's own copy of what it opens, filed beside the link itself.
                LinkAt::Book(book) => duplicate_book(state, book, id, name)
                    .await
                    .map(|name| Duplicated { name, shelf: false }),
                LinkAt::Dead => {
                    toast(
                        state,
                        format!("“{name}” points at a book that is not there any more."),
                    );
                    None
                }
            }
        }
        Row::Book(book) => {
            if book.missing {
                return None;
            }
            let shown = book.title();
            duplicate_book(state, book, id, shown)
                .await
                .map(|name| Duplicated { name, shelf: false })
        }
    }
}

/// What a link points at, for the duplicate it is about to become: the book
/// whose bytes the copy will wear, or the shelf a second pointer stays at. A
/// link whose target is gone, dead or not a book answers [`LinkAt::Dead`],
/// which is the refusal every caller of this shares.
enum LinkAt {
    Book(Book),
    Shelf,
    Dead,
}

fn link_at(rows: &[Row], target: &str) -> LinkAt {
    if id::is_shelf(target) {
        return LinkAt::Shelf;
    }
    match find_row(rows, target) {
        Some(Row::Book(book)) if !book.missing => LinkAt::Book(book.clone()),
        _ => LinkAt::Dead,
    }
}

/// A shelf link's duplicate: a second link at the same shelf, filed beside
/// the first. The counter name and the filing are the row rules every
/// duplicate follows; only the "stays a pointer" half is this function's
/// own.
fn duplicate_shelf_link(
    state: crate::context::LibraryContext,
    row_id: &str,
    name: &str,
    target: &str,
) -> String {
    let now = now_ms();
    let title = name_for(state, name);
    let dup = Row::link(id::next_id(now), title.clone(), target.to_string(), now);
    let dup_id = dup.id().to_string();
    state.library.books.update(|rows| rows.push(dup));
    file_beside(state, row_id, &dup_id);
    title
}

/// A book's duplicate: a second copy in the library's own store, wearing the
/// name the reader pointed at — the book's own display name, or the link's
/// name when a link was what was duplicated. Whatever origin the original
/// had, the copy is `Stored`; the highlights come along as the copy's own
/// list; and the row lands beside the row the reader pointed at, not beside
/// the original the link happened to read.
///
/// `shown` is the name to counter-step on the level, minted by the caller
/// because the two callers read it off two different rows.
async fn duplicate_book(
    state: crate::context::LibraryContext,
    book: Book,
    beside: &str,
    shown: String,
) -> Option<String> {
    let book_id = id::next_id(now_ms());
    // A card for the copy: the shell's beats for it need somewhere to land,
    // and the duplicate of a big document is a copy the reader is waiting on
    // like any other.
    let task = import::begin_task(state, shown.clone());
    let from = book.path().to_string();
    let (new_store, measured) = match ipc::copy_one(&task, &from, &book_id).await {
        Ok(pair) => pair,
        Err(message) => {
            import::fail_task(state, &task, message);
            return None;
        }
    };
    let title = name_for(state, &shown);
    let marks_of = book.id.clone();
    let dup = stored_copy(&book, book_id, new_store, measured, &title, now_ms());
    let dup_id = dup.id.clone();
    state.library.books.update(|rows| rows.push(Row::Book(dup)));
    // The highlights are half of what the reader put into the book: the copy
    // lands wearing its own list of them — same words at the same spots, ids
    // of their own — so the two books' marks are separate from the first
    // moment.
    storage::copy_gloss(&marks_of, &dup_id);
    file_beside(state, beside, &dup_id);
    import::finish_task(state, &task, 1, 0);
    Some(title)
}

/// The whole run's plan before any bytes move: the fresh subtree, and every
/// member of it resolved to the thing it will become. Pure on the state it
/// read, so the host tests can ask it directly — the store is the only half of
/// a duplicate that cannot run there.
struct TreePlan {
    /// The copy's own name, the level's counter.
    name: String,
    /// The shelf the reader pointed at: the run's card is labelled with what
    /// is being duplicated, the way a folder run is labelled with its folder.
    label: String,
    /// Where the fresh subtree splices in: right behind this shelf, the row
    /// rule every duplicate follows.
    original_id: String,
    /// The fresh subtree in the original's own order — new ids, the counter
    /// name on the root, everything `Virtual`, parents remapped — with `books`
    /// still holding the ORIGINAL member ids, because which of them survive is
    /// the landing's to say and only for the members that actually copied.
    shelves: Vec<Shelf>,
    /// Every member of the subtree in first-seen order, keyed by the id the
    /// original shelves hold.
    members: Vec<(String, Member)>,
}

/// One member of the subtree, resolved for the copy it becomes. A book filed
/// twice inside one tree is one member, not two: the copy mirrors the tree it
/// came from, and it is the ORIGINAL the copy never shares with.
enum Member {
    /// A book, or a link at one: the bytes the run will copy, the name the copy
    /// wears, and the row whose highlights ride along — landed under `new_id`,
    /// which is also the store's own name for the copy's item folder.
    Copy {
        new_id: String,
        book: Book,
        shown: String,
    },
    /// A link at a shelf: carried as a fresh link row, its target remapped onto
    /// the copy when the shelf it points at is inside the tree.
    Link(Row),
    /// A book whose address died, a link at one, a membership naming no row:
    /// nothing to copy, so the copy of the tree goes without it rather than
    /// refusing the whole shelf.
    Skip,
}

/// A shelf's duplicate: the plan, the one store batch for every copy it
/// owes, and the landing. The batch is one call so one card carries the whole
/// run, and a per-file failure is one toast naming the file — the other
/// members still land, which is the folder run's own answer to a locked
/// file.
async fn duplicate_shelf_row(
    state: crate::context::LibraryContext,
    shelf_id: &str,
) -> Option<String> {
    let plan = plan_the_tree(state, shelf_id)?;
    let requests: Vec<BookFileRequest> = plan
        .members
        .iter()
        .filter_map(|(_, what)| match what {
            Member::Copy { new_id, book, .. } => Some(BookFileRequest {
                from: book.path().to_string(),
                id: new_id.clone(),
            }),
            _ => None,
        })
        .collect();
    if requests.is_empty() {
        // A tree of links and dead rows copies nothing: no card, and the landing
        // is the whole run.
        return Some(land_the_tree(state, plan, &HashMap::new()));
    }
    let label = plan.label.clone();
    let task = import::begin_task(state, label);
    match ipc::store_books(&task, &requests).await {
        Err(message) => {
            import::fail_task(state, &task, message);
            None
        }
        Ok(results) => {
            let landed = import::partition_store_results(state, results, "files");
            let name = land_the_tree(state, plan, &landed);
            import::finish_task(state, &task, landed.len() as u32, 0);
            Some(name)
        }
    }
}

/// Read the tree, mint its copy, and resolve every member. The subtree walk,
/// the shelf-id map and the member resolution are one pass because they answer
/// one question: what would the copy of this tree be?
fn plan_the_tree(state: crate::context::LibraryContext, shelf_id: &str) -> Option<TreePlan> {
    let shelves = state.library.shelves.get_untracked();
    let original = shelves_ops::find(&shelves, shelf_id)?;
    let now = now_ms();
    let name = next_shelf_name(&shelves, original.parent.as_deref(), &original.name);

    let mut order: Vec<&Shelf> = vec![original];
    let mut seen: HashSet<String> = HashSet::from([original.id.clone()]);
    let mut stack: Vec<&str> = vec![original.id.as_str()];
    while let Some(parent) = stack.pop() {
        for child in shelves_ops::children_of(&shelves, Some(parent)) {
            if !seen.insert(child.id.clone()) {
                continue;
            }
            order.push(child);
            stack.push(child.id.as_str());
        }
    }

    let mut fresh: HashMap<String, String> = HashMap::with_capacity(order.len());
    for shelf in &order {
        fresh.insert(shelf.id.clone(), id::next_shelf_id(now));
    }
    let skeleton: Vec<Shelf> = order
        .iter()
        .enumerate()
        .map(|(at, shelf)| Shelf {
            id: fresh.get(&shelf.id).cloned().unwrap_or_default(),
            name: if at == 0 {
                name.clone()
            } else {
                shelf.name.clone()
            },
            kind: ShelfKind::Virtual,
            books: shelf.books.clone(),
            // The subtree keeps its shape and closes no loop; a parent the
            // walk could not reach is no parent at all rather than an edge at
            // the ORIGINAL.
            parent: if at == 0 {
                original.parent.clone()
            } else {
                shelf.parent.as_deref().and_then(|p| fresh.get(p).cloned())
            },
            manual_parent: false,
        })
        .collect();

    let rows = state.library.books.get_untracked();
    let mut members: Vec<(String, Member)> = Vec::new();
    let mut taken: HashSet<String> = HashSet::new();
    for shelf in &order {
        for member in &shelf.books {
            if !taken.insert(member.clone()) {
                continue;
            }
            let what = match find_row(&rows, member) {
                Some(Row::Book(book)) if !book.missing => Member::Copy {
                    new_id: id::next_id(now),
                    book: book.clone(),
                    // The fresh levels hold nothing to collide with, so the copy
                    // wears the original's own display name: the tree mirrors the
                    // tree, counter names and all.
                    shown: book.title(),
                },
                Some(Row::Link { name, target, .. }) => match link_at(&rows, target) {
                    LinkAt::Book(book) => Member::Copy {
                        new_id: id::next_id(now),
                        book,
                        shown: name.clone(),
                    },
                    LinkAt::Shelf => Member::Link(Row::link(
                        id::next_id(now),
                        name.clone(),
                        // A link at a shelf inside the tree points at the copy of
                        // that shelf; a link at one outside it keeps pointing where
                        // it did, because that shelf is nobody's to duplicate here.
                        fresh.get(target).cloned().unwrap_or_else(|| target.clone()),
                        now,
                    )),
                    LinkAt::Dead => Member::Skip,
                },
                _ => Member::Skip,
            };
            members.push((member.clone(), what));
        }
    }

    Some(TreePlan {
        name,
        label: original.name.clone(),
        original_id: original.id.clone(),
        shelves: skeleton,
        members,
    })
}

/// Land the plan: the rows the copies and carried links become, the highlights
/// riding onto each copy, the member lists rewritten onto the fresh ids, and
/// the fresh tree spliced in right behind the original. `landed` is the store
/// batch's answer — which copies came home, and each one's own measurement —
/// and a member whose copy is not in it is dropped, not shared.
fn land_the_tree(
    state: crate::context::LibraryContext,
    plan: TreePlan,
    landed: &HashMap<String, (String, Option<Fingerprint>)>,
) -> String {
    let now = now_ms();
    let TreePlan {
        name,
        original_id,
        shelves: mut fresh,
        members,
        ..
    } = plan;
    let mut mapped: HashMap<String, String> = HashMap::with_capacity(members.len());
    let mut rows: Vec<Row> = Vec::with_capacity(members.len());
    for (old, what) in &members {
        match what {
            Member::Copy {
                new_id,
                book,
                shown,
            } => {
                let Some((store, measured)) = landed.get(new_id) else {
                    continue;
                };
                let dup = stored_copy(book, new_id.clone(), store.clone(), *measured, shown, now);
                storage::copy_gloss(&book.id, new_id);
                mapped.insert(old.clone(), new_id.clone());
                rows.push(Row::Book(dup));
            }
            Member::Link(row) => {
                mapped.insert(old.clone(), row.id().to_string());
                rows.push(row.clone());
            }
            Member::Skip => {}
        }
    }
    if !rows.is_empty() {
        state.library.books.update(|list| list.extend(rows));
    }
    state.library.shelves.update(|live| {
        for shelf in &mut fresh {
            let original_members = std::mem::take(&mut shelf.books);
            shelf.books = original_members
                .into_iter()
                .filter_map(|old| mapped.get(&old).cloned())
                .collect();
        }
        // Right behind the original's own row: the shelf list IS the render
        // order, so a copy appended to the end would be a shelf the reader
        // has to go and find.
        let at = live
            .iter()
            .position(|s| s.id == original_id)
            .map_or(live.len(), |at| at + 1);
        live.splice(at..at, fresh);
    });
    name
}

/// The copy of a book, which is the same object whichever door duplicated it:
/// a stored book at a fresh id, the original's address kept as where the bytes
/// came from, and the name the reader asked for rather than the file's own.
///
/// `shown` is worn as a locked title — a base the file itself carried
/// underscores in ("harry_potter_1") reads as snake-case debris to the title
/// rule, and a name the reader just asked for is not debris.
fn stored_copy(
    book: &Book,
    new_id: String,
    store: String,
    measured: Option<Fingerprint>,
    shown: &str,
    now: u64,
) -> Book {
    let mut dup = Book::new(
        new_id,
        Fingerprint::placeholder(&store),
        book.format,
        Origin::Stored {
            src: book.origin.source().map(str::to_string),
            store,
        },
        now,
    );
    dup.title = Some(shown.to_string());
    dup.title_locked = true;
    dup.adopt_measurement(measured);
    dup
}

// The file manager's counter, counted against the level the reader clicked
// from: the collision that matters is the one they can see.
fn name_for(state: crate::context::LibraryContext, display: &str) -> String {
    let (rows, shelves) = (
        state.library.books.get_untracked(),
        state.library.shelves.get_untracked(),
    );
    let level = state.library.shelf.get_untracked();
    next_name(&rows, &shelves, &level, display)
}

/// `place` is the drag's spelling — remove, insert at the index — which is
/// what "beside the row you pointed at" means on a list the reader can see.
fn file_beside(state: crate::context::LibraryContext, original_id: &str, dup_id: &str) {
    let seats: Vec<(String, usize)> = state.library.shelves.with_untracked(|shelves| {
        shelves_ops::containing(shelves, original_id)
            .into_iter()
            .filter_map(|shelf| {
                let at = shelf.books.iter().position(|m| m == original_id)?;
                Some((shelf.id.clone(), at + 1))
            })
            .collect()
    });
    if seats.is_empty() {
        return;
    }
    state.library.shelves.update(|shelves| {
        for (shelf_id, index) in &seats {
            if let Some(shelf) = shelves_ops::find_mut(shelves, shelf_id) {
                shelves_ops::place(&mut shelf.books, dup_id, Some(*index));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::LibraryContext;

    #[test]
    fn a_link_at_a_shelf_duplicates_as_a_link_beside_it() {
        let owner = Owner::new();
        owner.set();
        let state = LibraryContext::default();
        state.library.books.set(vec![
            library_core::testkit::row_at("b1", "/books/dune.md"),
            library_core::testkit::link("l1", "Dune", "s1"),
        ]);
        let mut shelf = library_core::testkit::plain_shelf("s1", &["b1", "l1"]);
        shelf.name = "Shelf".into();
        state.library.shelves.set(vec![shelf]);
        state.library.shelf.set("s1".to_string());

        let name = duplicate_shelf_link(state, "l1", "Dune", "s1");
        assert_eq!(name, "Dune_1", "the level's counter, not a collision");
        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 3);
        let dup = rows
            .iter()
            .find(|r| r.id() != "b1" && r.id() != "l1")
            .unwrap();
        match dup {
            Row::Link { target, name, .. } => {
                assert_eq!(target, "s1", "the pointer points where the pointer pointed");
                assert_eq!(name, "Dune_1");
            }
            Row::Book(_) => panic!("a shelf's link duplicates as a link"),
        }
        let shelves = state.library.shelves.get_untracked();
        assert_eq!(
            shelves[0].books,
            vec!["b1".to_string(), "l1".to_string(), dup.id().to_string()],
            "filed right behind the row the reader pointed at"
        );
    }

    #[test]
    fn a_link_answers_with_what_it_points_at() {
        let rows = vec![
            library_core::testkit::row("b1"),
            library_core::testkit::link("l1", "Dune", "b1"),
            library_core::testkit::link("ls", "Shelf door", "s1"),
            library_core::testkit::link("dead", "Gone", "b9"),
        ];
        let mut missing = library_core::testkit::book("b2");
        missing.missing = true;
        let rows = [rows, vec![Row::Book(missing)]].concat();

        assert!(matches!(link_at(&rows, "s1"), LinkAt::Shelf));
        match link_at(&rows, "b1") {
            LinkAt::Book(book) => assert_eq!(book.id, "b1"),
            _ => panic!("a live book is a copy to make"),
        }
        assert!(
            matches!(link_at(&rows, "b9"), LinkAt::Dead),
            "a target no row answers for"
        );
        assert!(
            matches!(link_at(&rows, "b2"), LinkAt::Dead),
            "a target whose address died"
        );
        assert!(
            matches!(link_at(&rows, "l1"), LinkAt::Dead),
            "a link at a link"
        );
    }

    fn nested_state() -> (LibraryContext, Owner) {
        let owner = Owner::new();
        owner.set();
        let state = LibraryContext::default();
        state.library.books.set(vec![
            library_core::testkit::markdown_row("b1"),
            library_core::testkit::markdown_row("b2"),
        ]);
        state.library.shelves.set(vec![
            library_core::testkit::shelf("s1", "Shelf", &["b1"], None),
            library_core::testkit::shelf("s2", "Inside", &["b2"], Some("s1")),
            library_core::testkit::shelf("s3", "Other", &[], None),
        ]);
        (state, owner)
    }

    fn copy_of(shelves: &[Shelf], name: &str) -> Shelf {
        shelves
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("no shelf called {name}"))
            .clone()
    }

    /// The batch the store would have answered with, fabricated for the members
    /// a plan wants to copy: every copy lands, wearing its own item folder and
    /// its own measurement.
    fn everything_landed(plan: &TreePlan) -> HashMap<String, (String, Option<Fingerprint>)> {
        plan.members
            .iter()
            .filter_map(|(_, what)| match what {
                Member::Copy { new_id, .. } => Some((
                    new_id.clone(),
                    (
                        format!("/app/Library/items/{new_id}/source.md"),
                        Some(library_core::testkit::fp_n(7)),
                    ),
                )),
                _ => None,
            })
            .collect()
    }

    fn copies_of(plan: &TreePlan) -> Vec<&Member> {
        plan.members
            .iter()
            .map(|(_, what)| what)
            .filter(|what| matches!(what, Member::Copy { .. }))
            .collect()
    }

    #[test]
    fn a_tree_plan_copies_every_member_once() {
        let (state, _owner) = nested_state();
        let plan = plan_the_tree(state, "s1").expect("the shelf is there");
        assert_eq!(plan.name, "Shelf_1", "the level's counter, not a collision");
        assert_eq!(plan.label, "Shelf", "the card names what was duplicated");

        // One member per row, first-seen order: b1 off the root, b2 off the rung inside.
        let order: Vec<&str> = plan.members.iter().map(|(old, _)| old.as_str()).collect();
        assert_eq!(order, vec!["b1", "b2"]);
        let copies = copies_of(&plan);
        assert_eq!(copies.len(), 2, "every member becomes a copy");
        let ids: Vec<&str> = plan
            .members
            .iter()
            .filter_map(|(_, what)| match what {
                Member::Copy { new_id, .. } => Some(new_id.as_str()),
                _ => None,
            })
            .collect();
        assert_ne!(ids[0], "b1", "the copy is a row of its own");
        assert_ne!(ids[1], "b2", "the copy is a row of its own");
        assert_ne!(ids[0], ids[1], "two members, two copies");
    }

    #[test]
    fn a_missing_book_and_a_dead_link_are_skipped_not_shared() {
        let (state, _owner) = nested_state();
        let mut missing = library_core::testkit::markdown_book("b3");
        missing.missing = true;
        state.library.books.update(|rows| {
            rows.push(Row::Book(missing));
            rows.push(library_core::testkit::link("dead", "Gone", "b9"));
        });
        state.library.shelves.update(|shelves| {
            shelves[0].books = vec!["b1".into(), "b3".into(), "dead".into()];
        });

        let plan = plan_the_tree(state, "s1").expect("the shelf is there");
        let kinds: Vec<&str> = plan
            .members
            .iter()
            .map(|(_, what)| match what {
                Member::Copy { .. } => "copy",
                Member::Link(_) => "link",
                Member::Skip => "skip",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["copy", "skip", "skip", "copy"],
            "the live books copy; the dead rows are no one's to share"
        );
    }

    #[test]
    fn a_link_at_a_shelf_inside_the_tree_points_at_the_copy() {
        let (state, _owner) = nested_state();
        state.library.books.update(|rows| {
            rows.push(library_core::testkit::link("l2", "Inside door", "s2"));
            rows.push(library_core::testkit::link("l3", "Other door", "s3"));
        });
        state.library.shelves.update(|shelves| {
            shelves[0].books.push("l2".into());
            shelves[0].books.push("l3".into());
        });

        let plan = plan_the_tree(state, "s1").expect("the shelf is there");
        let fresh_inside = plan.shelves[1].id.clone();
        let targets: Vec<(String, String)> = plan
            .members
            .iter()
            .filter_map(|(old, what)| match what {
                Member::Link(row) => Some((old.clone(), row.target().unwrap().to_string())),
                _ => None,
            })
            .collect();
        assert_eq!(
            targets.len(),
            2,
            "both links came along, each as its own row"
        );
        assert_eq!(
            targets[0],
            ("l2".to_string(), fresh_inside),
            "a link at a shelf inside the tree points at the copy of that shelf"
        );
        assert_ne!(targets[0].1, "s2", "remapped, not shared");
        assert_eq!(
            targets[1],
            ("l3".to_string(), "s3".to_string()),
            "a link at a shelf outside the tree keeps pointing where it did"
        );
    }

    #[test]
    fn the_landing_remaps_the_tree_onto_fresh_copies() {
        let (state, _owner) = nested_state();
        let plan = plan_the_tree(state, "s1").expect("the shelf is there");
        let landed = everything_landed(&plan);
        let name = land_the_tree(state, plan, &landed);

        assert_eq!(name, "Shelf_1");
        let shelves = state.library.shelves.get_untracked();
        assert_eq!(shelves.len(), 5, "the shelf and the one inside it, copied");
        let root = copy_of(&shelves, "Shelf_1");
        assert_eq!(
            root.books.len(),
            1,
            "one member, and it is not the original's"
        );
        let fresh_id = root.books[0].clone();
        assert_ne!(
            fresh_id, "b1",
            "the tree holds its own copy, not the row itself"
        );

        let rows = state.library.books.get_untracked();
        let dup = library_core::book::find_by_id(&rows, &fresh_id).expect("the copy's row");
        match &dup.origin {
            Origin::Stored { src, store } => {
                assert_eq!(
                    src.as_deref(),
                    Some("/books/b1.md"),
                    "read at its place, and the copy is the library's own anyway"
                );
                assert_eq!(
                    store,
                    &format!("/app/Library/items/{fresh_id}/source.md"),
                    "the copy's item folder is its own"
                );
            }
            other => panic!("the duplicate is stored, whatever the original was: {other:?}"),
        }
        assert_eq!(
            dup.title.as_deref(),
            Some("b1"),
            "the name the original showed"
        );
        assert!(
            dup.title_locked,
            "a name the reader asked for is not debris"
        );
        assert_eq!(
            dup.fp,
            library_core::testkit::fp_n(7),
            "known by its own measurement"
        );
        assert!(
            !dup.fp_pending,
            "the measurement the copy rode home with landed"
        );

        let inside = copy_of(&shelves, "Inside");
        assert_eq!(inside.books.len(), 1);
        assert_ne!(inside.books[0], "b2", "the rung's copy is its own row too");
        assert_eq!(
            inside.parent.as_deref(),
            Some(root.id.as_str()),
            "still inside the copy, not the original"
        );
        let level: Vec<&str> = library_core::shelf::children_of(&shelves, None)
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            level,
            vec!["Shelf", "Shelf_1", "Other"],
            "spliced in behind the original"
        );
    }

    #[test]
    fn a_member_the_store_refused_is_dropped_not_shared() {
        let (state, _owner) = nested_state();
        let plan = plan_the_tree(state, "s1").expect("the shelf is there");
        let before = state.library.books.get_untracked().len();
        let name = land_the_tree(state, plan, &HashMap::new());

        assert_eq!(name, "Shelf_1", "the shelf itself still landed");
        let shelves = state.library.shelves.get_untracked();
        assert_eq!(shelves.len(), 5, "the tree keeps its shape");
        let root = copy_of(&shelves, "Shelf_1");
        assert!(
            root.books.is_empty(),
            "a copy that did not come home is not filed as the original"
        );
        assert_eq!(
            state.library.books.get_untracked().len(),
            before,
            "no row landed for a copy that did not"
        );
    }

    #[test]
    fn a_folder_shelf_duplicates_as_a_shelf_of_the_readers_own() {
        // One directory is one linked shelf: a second folder shelf of one rung
        // would be two doors to one directory with only one of them on the
        // ledger — and a rescan would re-hang a shelf the reader made. The
        // copy is the reader's own second tree over fresh copies of the
        // books.
        let (state, _owner) = nested_state();
        state.library.shelves.update(|shelves| {
            shelves[0] =
                library_core::testkit::folder_shelf("s1", "Books", "f1", None, &["b1"], None);
        });
        let plan = plan_the_tree(state, "s1").expect("the shelf is there");
        assert_eq!(plan.name, "Books_1");
        assert!(
            plan.shelves[0].kind.folder_id().is_none(),
            "the copy is not a second shelf of the folder"
        );
        let landed = everything_landed(&plan);
        land_the_tree(state, plan, &landed);

        let shelves = state.library.shelves.get_untracked();
        let root = copy_of(&shelves, "Books_1");
        assert_ne!(
            root.books[0], "b1",
            "the folder's copy holds a copy, not the row"
        );
        // The original is untouched: the copy is not a rung of the folder, so a
        // walk that mints the tree again mints the tree it already had.
        let original = library_core::shelf::find(&shelves, "s1").expect("the original stands");
        assert_eq!(original.kind.folder_id(), Some("f1"));
        assert_eq!(original.books, vec!["b1".to_string()]);
    }

    #[test]
    fn a_duplicate_of_a_duplicate_steps_the_shelf_counter() {
        let (state, _owner) = nested_state();
        let first = plan_the_tree(state, "s1").expect("the shelf is there");
        assert_eq!(first.name, "Shelf_1");
        let first_id = first.shelves[0].id.clone();
        land_the_tree(state, first, &HashMap::new());
        // Duplicating the copy steps rather than stacks, the reading a file
        // manager gives: the counter is not part of the name.
        assert_eq!(
            plan_the_tree(state, &first_id).map(|plan| plan.name),
            Some("Shelf_2".to_string())
        );
    }

    #[test]
    fn a_shelf_that_is_not_there_duplicates_into_nothing() {
        let (state, _owner) = nested_state();
        assert!(plan_the_tree(state, "gone").is_none());
        // "All" is the book list and not a shelf, so it has no second instance
        // to make either — the pseudo-shelf's own answer everywhere else.
        assert!(plan_the_tree(state, library_core::shelf::ALL_SHELF).is_none());
        assert_eq!(state.library.shelves.get_untracked().len(), 3);
    }

    #[test]
    fn a_report_counts_the_kinds_it_landed() {
        let one = |name: &str, shelf| Duplicated {
            name: name.to_string(),
            shelf,
        };
        assert_eq!(report(&[one("Dune_1", false)]), "Duplicated as “Dune_1”.");
        assert_eq!(report(&[one("Shelf_1", true)]), "Duplicated as “Shelf_1”.");
        assert_eq!(
            report(&[one("a", false), one("b", false)]),
            "Duplicated 2 books."
        );
        assert_eq!(
            report(&[one("a", true), one("b", true)]),
            "Duplicated 2 shelves."
        );
        assert_eq!(
            report(&[one("a", true), one("b", false), one("c", false)]),
            "Duplicated 3 shelves and books."
        );
    }
}
