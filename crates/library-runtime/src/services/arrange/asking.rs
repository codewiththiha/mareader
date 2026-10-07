//! The one question every copy asks: leaving the ground stores a copy.

use std::collections::HashSet;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::find_row;
use library_core::shelf::{self as shelf, Shelf};
use library_core::text::plural;

use crate::services::folder_label;

use super::ReadingData;
use super::departure::{converting_rows, depart};
use super::moves::RowMove;
use super::purge_books;
use super::shelf_apart::take_the_rung_apart;
use super::shelf_departure::{
    ReturnPath, ShelfSeam, departing_book_ids, departing_sets, landing_level, return_path,
    take_shelves_out, take_them_home, target_is_family,
};
use super::shelves::delete_shelf;

/// The action's own wording: "1 Take shelf apart" is not a sentence.
fn doing(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

/// The reader's gesture, and what the answer needs to finish it.
#[derive(Clone, PartialEq)]
enum CopyWork {
    /// Books on the move, and the way back into the gesture that held them.
    Rows { ids: Vec<String>, hand: RowMove },
    /// Shelves off their tree's seat, their level, and their way home.
    Shelf {
        ids: Vec<String>,
        target: Option<String>,
        seam: Option<ShelfSeam>,
        returns: Vec<(String, ReturnPath)>,
    },
    /// A rung of a read-at-place tree coming apart.
    Rung { id: String },
    /// The removal's own gesture: books out, shelves off the list.
    Removal {
        purge: Vec<String>,
        shelves: Vec<String>,
        data: ReadingData,
    },
}

/// One answer's button, built at the raise: the wording is a fact about the
/// gesture.
#[derive(Clone, PartialEq)]
pub struct CopyOption {
    pub label: String,
    pub title: String,
    pub answer: CopyAnswer,
    pub primary: bool,
}

/// The reader's answer: copy, finish without them, or cancel.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CopyAnswer {
    Copy,
    WithoutCopies,
    Cancel,
}

/// The question, ready for the sheet: subject, cost and answers.
#[derive(Clone, PartialEq)]
pub struct CopyAsk {
    /// The action the reader is in the middle of, in their own words.
    pub action: String,
    /// The shelf or the books it is about.
    pub subject: String,
    /// The cost, in one or two lines.
    pub lines: Vec<String>,
    pub options: Vec<CopyOption>,
    work: CopyWork,
}

const UNTOUCHED: &str = "The folder on disk is untouched.";

impl CopyAsk {
    /// A book move: the rows the gate screened in place, and their ground.
    fn of_rows(
        state: crate::context::LibraryContext,
        ids: &[String],
        hand: RowMove,
    ) -> Option<Self> {
        let books = ids.len();
        let folder = ground_of(state, ids)?;
        Some(Self {
            action: "Move books".to_string(),
            subject: format!("{} from “{folder}”", plural(books, "book", "books")),
            lines: vec![
                plural(
                    books,
                    "It is read in place; moving it out stores a copy.",
                    "They are read in place; moving them out stores copies.",
                ),
                UNTOUCHED.to_string(),
            ],
            options: vec![copying("Copy and move")],
            work: CopyWork::Rows {
                ids: ids.to_vec(),
                hand,
            },
        })
    }

    /// Shelves off their tree's seat: the level and its in-place books go.
    pub(super) fn of_shelf(
        state: crate::context::LibraryContext,
        departing: Vec<String>,
        target: Option<String>,
        seam: Option<ShelfSeam>,
    ) -> Option<Self> {
        let shelves = state.library.shelves.get_untracked();
        let folders = state.library.folders.get_untracked();
        let books = state.library.books.get_untracked();
        let level = landing_level(&shelves, &target, seam.as_ref());
        let mut promised: HashSet<String> = shelf::children_of(&shelves, level.as_deref())
            .into_iter()
            .map(|s| s.name.clone())
            .collect();
        let mut names: Vec<String> = Vec::new();
        let mut copies: Vec<String> = Vec::new();
        let mut returns: Vec<(String, ReturnPath)> = Vec::new();
        // The first folder that placed one of the books names the sheet.
        let mut folder = String::new();
        let mut count = 0usize;
        for id in &departing {
            let Some(one) = shelf::find(&shelves, id) else {
                continue;
            };
            let Some(folder_id) = one.kind.folder_id() else {
                continue;
            };
            let Some(placing) = folders.iter().find(|f| f.id == folder_id) else {
                continue;
            };
            let (subtree, rungs) = departing_sets(&shelves, folder_id, id);
            count += departing_book_ids(&books, &shelves, placing, &rungs, &subtree).len();
            if folder.is_empty() {
                folder = folder_label(&placing.root);
            }
            // Offered only for a drop inside the mover's family.
            let ground = library_core::folder::dir_of_rung(&placing.root, one.kind.rung());
            if target_is_family(&shelves, &folders, level.as_deref(), &ground)
                && let Some(path) = return_path(&shelves, &folders, id)
            {
                returns.push((id.clone(), path));
            }
            copies.push(free_name(&one.name, &mut promised));
            names.push(one.name.clone());
        }
        if names.is_empty() {
            return None;
        }
        let one = names.len() == 1;
        let cost = plural(count, "book", "books");
        let subject = match (one, count) {
            (true, 0) => format!("“{}” — nothing read in place", names[0]),
            (true, _) => format!("“{}” — {cost} from “{folder}”", names[0]),
            (false, 0) => plural(names.len(), "shelf", "shelves"),
            (false, _) => format!(
                "{} — {cost} read in place",
                plural(names.len(), "shelf", "shelves")
            ),
        };
        let lines = match (one, count) {
            (true, 0) => vec![
                "Nothing on it is read in place, so no file is copied.".to_string(),
                UNTOUCHED.to_string(),
            ],
            (true, _) => vec![
                format!(
                    "Moving it out stores its {cost}; the copy lands as “{}”.",
                    copies[0]
                ),
                UNTOUCHED.to_string(),
            ],
            (false, 0) => vec![
                "Nothing on them is read in place, so no file is copied.".to_string(),
                UNTOUCHED.to_string(),
            ],
            (false, _) => vec![
                format!(
                    "Moving them out stores their {cost}; the copies land as {}.",
                    listed(&copies)
                ),
                UNTOUCHED.to_string(),
            ],
        };
        let mut options = vec![copying("Copy and move")];
        if !returns.is_empty() {
            options.push(CopyOption {
                label: if returns.len() == 1 {
                    "Put it back in its place".to_string()
                } else {
                    "Put them back in their places".to_string()
                },
                title: "Return each shelf to the place its folder names; nothing is copied"
                    .to_string(),
                answer: CopyAnswer::WithoutCopies,
                primary: false,
            });
        }
        Some(Self {
            action: doing(names.len(), "Move shelf", "Move shelves"),
            subject,
            lines,
            options,
            work: CopyWork::Shelf {
                ids: departing,
                target,
                seam,
                returns,
            },
        })
    }

    /// A rung coming apart: its books become copies before it goes.
    pub(super) fn of_apart(state: crate::context::LibraryContext, shelf_id: &str) -> Option<Self> {
        let books = books_the_rung_takes(state, shelf_id);
        if books.is_empty() {
            return None;
        }
        let folder = ground_of(state, &books)?;
        let shelves = state.library.shelves.get_untracked();
        let rung = shelf::find(&shelves, shelf_id)?;
        let home = match rung
            .kind
            .folder_id()
            .and_then(|folder_id| shelf::rung_above(&shelves, folder_id, rung.kind.rung()))
            .and_then(|seat| shelf::find(&shelves, &seat))
        {
            Some(seat) => format!("“{}”", seat.name),
            None => "the library's top level".to_string(),
        };
        let cost = plural(books.len(), "book", "books");
        let line = if books.len() == 1 {
            format!("The book read in place here becomes a copy and comes up to {home}.")
        } else {
            format!(
                "The {} books read in place here become copies and come up to {home}.",
                books.len()
            )
        };
        Some(Self {
            action: "Take shelf apart".to_string(),
            subject: format!("“{}” — {cost} from “{folder}”", rung.name),
            lines: vec![line, UNTOUCHED.to_string()],
            options: vec![copying("Copy and take apart")],
            work: CopyWork::Rung {
                id: shelf_id.to_string(),
            },
        })
    }

    /// A shelf coming off the list with in-place books: the same copy.
    fn of_removal(
        state: crate::context::LibraryContext,
        purge: &[String],
        shelves: &[String],
        data: ReadingData,
    ) -> Option<Self> {
        let taking = shelf_books(state, shelves, purge);
        if taking.is_empty() {
            return None;
        }
        let folder = ground_of(state, &taking)?;
        let names: Vec<String> = shelves
            .iter()
            .filter_map(|id| {
                state
                    .library
                    .shelves
                    .with_untracked(|all| shelf::find(all, id).map(|s| s.name.clone()))
            })
            .collect();
        let one = names.len() == 1;
        let again = if one {
            format!("Or let “{folder}” make it again.")
        } else {
            "Or let the folders make them again.".to_string()
        };
        let label = if one {
            format!("Let “{folder}” make it again")
        } else {
            "Let the folders make them again".to_string()
        };
        let kept = if one {
            plural(
                taking.len(),
                "Read in place, so removing it stores a copy.",
                "Read in place, so removing it stores copies.",
            )
        } else {
            plural(
                taking.len(),
                "Read in place, so removing them stores a copy.",
                "Read in place, so removing them stores copies.",
            )
        };
        let lines = vec![kept, again];
        Some(Self {
            action: doing(names.len(), "Remove shelf", "Remove shelves"),
            subject: plural(names.len(), "shelf", "shelves"),
            lines,
            options: vec![
                copying("Copy and remove"),
                CopyOption {
                    label,
                    title: "Take the shelf off the list; its books read in place where they are"
                        .to_string(),
                    answer: CopyAnswer::WithoutCopies,
                    primary: false,
                },
            ],
            work: CopyWork::Removal {
                purge: purge.to_vec(),
                shelves: shelves.to_vec(),
                data,
            },
        })
    }
}

fn copying(label: &str) -> CopyOption {
    CopyOption {
        label: label.to_string(),
        title: "Copy the books read in place into the library, then finish".to_string(),
        answer: CopyAnswer::Copy,
        primary: true,
    }
}

/// The copy names as the sheet lists them.
fn listed(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format!("“{name}”"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A copy takes the next free name, leaving the folder's own name free.
pub(super) fn free_name(name: &str, promised: &mut HashSet<String>) -> String {
    let free = library_core::book::duplicate_title(name, promised);
    promised.insert(free.clone());
    free
}

/// The books a rung takes: placed by the folder, standing inside it.
pub(super) fn books_the_rung_takes(
    state: crate::context::LibraryContext,
    shelf_id: &str,
) -> Vec<String> {
    let shelves = state.library.shelves.get_untracked();
    let folders = state.library.folders.get_untracked();
    let books = state.library.books.get_untracked();
    let Some(rung) = shelf::find(&shelves, shelf_id) else {
        return Vec::new();
    };
    let Some(folder) = rung
        .kind
        .folder_id()
        .and_then(|id| folders.iter().find(|f| f.id == id))
    else {
        return Vec::new();
    };
    if !folder.mode().reads_in_place() {
        return Vec::new();
    }
    let (subtree, _) = departing_sets(&shelves, &folder.id, shelf_id);
    let going: HashSet<String> = std::iter::once(shelf_id.to_string()).collect();
    departing_book_ids(&books, &shelves, folder, &going, &subtree)
}

/// Every book the named shelves take, and none the removal takes anyway.
fn shelf_books(
    state: crate::context::LibraryContext,
    shelves: &[String],
    purge: &[String],
) -> Vec<String> {
    let mut taking: Vec<String> = Vec::new();
    for id in shelves {
        for book in books_the_rung_takes(state, id) {
            if !purge.contains(&book) && !taking.contains(&book) {
                taking.push(book);
            }
        }
    }
    taking
}

/// The folder whose ground these books leave, named by the sheet.
fn ground_of(state: crate::context::LibraryContext, ids: &[String]) -> Option<String> {
    let books = state.library.books.get_untracked();
    let folders = state.library.folders.get_untracked();
    ids.iter().find_map(|id| {
        let fp = find_row(&books, id)
            .and_then(|row| row.book())
            .map(|b| b.fp)?;
        let folder = folders
            .iter()
            .find(|f| f.mode().reads_in_place() && f.placed.contains(&fp))?;
        Some(folder_label(&folder.root))
    })
}

/// The gate every hand-move rides: a copy is a question. `true` means wait.
pub(super) fn ask_move_copy(
    state: crate::context::LibraryContext,
    ids: &[String],
    to: &str,
    hand: RowMove,
) -> bool {
    if !tauri_bridge::has_tauri() {
        return false;
    }
    let converting = converting_rows(state, ids, to);
    let Some(ask) = CopyAsk::of_rows(state, &converting, hand) else {
        return false;
    };
    raise(state, ask);
    true
}

/// The shelf half: one question per gesture, for named seats only.
pub(super) fn ask_move_shelf(
    state: crate::context::LibraryContext,
    departing: Vec<String>,
    target: Option<String>,
    seam: Option<ShelfSeam>,
) {
    if let Some(ask) = CopyAsk::of_shelf(state, departing, target, seam) {
        raise(state, ask);
    }
}

/// The removal's own door: in-place books buy copies first.
pub fn remove_entries(
    state: crate::context::LibraryContext,
    purge: Vec<String>,
    shelves: Vec<String>,
    data: ReadingData,
) {
    match CopyAsk::of_removal(state, &purge, &shelves, data) {
        Some(ask) => raise(state, ask),
        None => remove(state, &purge, &shelves, data),
    }
}

pub(super) fn raise(state: crate::context::LibraryContext, ask: CopyAsk) {
    state.library.copy_ask.raise(ask);
}

/// Nothing moves and nothing copies.
pub fn cancel_copy(state: crate::context::LibraryContext) {
    state.library.copy_ask.dismiss();
}

/// The sheet's own answer, one entry per button, so the view holds no logic.
pub fn answer_copy(state: crate::context::LibraryContext, answer: CopyAnswer) {
    let Some(ask) = state.library.copy_ask.ask.get_untracked() else {
        return;
    };
    cancel_copy(state);
    match answer {
        CopyAnswer::Cancel => {}
        CopyAnswer::WithoutCopies => finish_without(state, ask),
        CopyAnswer::Copy => {
            if !tauri_bridge::has_tauri() {
                return;
            }
            spawn_local(async move {
                copy_and_finish(state, ask).await;
            });
        }
    }
}

/// No copies: a shelf with a way home takes it, a removal runs as always.
fn finish_without(state: crate::context::LibraryContext, ask: CopyAsk) {
    match ask.work {
        CopyWork::Shelf { returns, .. } => take_them_home(state, &returns),
        CopyWork::Removal {
            purge,
            shelves,
            data,
        } => remove(state, &purge, &shelves, data),
        CopyWork::Rows { .. } | CopyWork::Rung { .. } => {}
    }
}

/// The copies run in a spawned task, so the sheet leaves the screen at once.
async fn copy_and_finish(state: crate::context::LibraryContext, ask: CopyAsk) {
    match ask.work {
        CopyWork::Rows { ids, hand } => {
            let converting = converting_rows(state, &ids, hand.to());
            let copies = depart(state, &converting).await;
            // A failed copy costs that book its move and nothing else.
            let failed: Vec<String> = converting
                .iter()
                .filter(|id| !copies.contains(id))
                .cloned()
                .collect();
            let rest: Vec<String> = ids.into_iter().filter(|id| !failed.contains(id)).collect();
            if !rest.is_empty() {
                hand.resume(state, rest, copies);
            }
        }
        CopyWork::Shelf {
            ids, target, seam, ..
        } => take_shelves_out(state, ids, target, seam).await,
        CopyWork::Rung { id } => take_the_rung_apart(state, id).await,
        CopyWork::Removal {
            purge,
            shelves,
            data,
        } => {
            depart(state, &shelf_books(state, &shelves, &purge)).await;
            remove(state, &purge, &shelves, data);
        }
    }
}

/// A removal, whole: shelves come off deepest first, or no sweep reaches them.
fn remove(
    state: crate::context::LibraryContext,
    purge: &[String],
    shelves: &[String],
    data: ReadingData,
) {
    if !purge.is_empty() {
        purge_books(state, purge, data);
    }
    let all: Vec<Shelf> = state.library.shelves.get_untracked();
    let mut going: Vec<(usize, String)> = shelves
        .iter()
        .map(|id| (shelf::ancestors(&all, id).len(), id.clone()))
        .collect();
    going.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
    for (_, id) in going {
        delete_shelf(state, &id);
    }
}
