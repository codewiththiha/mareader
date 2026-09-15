//! The one question every copy asks: a book or a shelf the library reads in
//! place leaves the ground that made it, becomes the library's own stored
//! copy, and a copy is a cost the reader agrees to. One sheet for every door
//! in, so no path stores a file in silence.

use std::collections::HashSet;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::find_row;
use library_core::shelf::{self as shelf, Shelf};
use library_core::text::plural;

use crate::services::library::folder_label;
use crate::state::AppState;

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

/// The action's own wording: the count belongs to the subject, and "1 Take
/// shelf apart" is not a sentence. `text::plural` counts because a count is
/// what a subject is for; this one refuses it for the same reason, and the
/// two are not one helper.
fn doing(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

/// What the reader is in the middle of, and everything the answer needs to
/// finish it. One enum rather than a flag per door: the sheet names the
/// action, and the answer resumes THAT gesture rather than a second one built
/// from the same facts.
#[derive(Clone, PartialEq)]
enum CopyWork {
    /// Books on the move, and the way back into the drag or filing that held them.
    Rows { ids: Vec<String>, hand: RowMove },
    /// Shelves off the seat their folder's tree names, the level they were
    /// dropped on, and the ones with a way home instead of a copy.
    Shelf {
        ids: Vec<String>,
        target: Option<String>,
        seam: Option<ShelfSeam>,
        returns: Vec<(String, ReturnPath)>,
    },
    /// A rung of a read-at-place tree coming apart.
    Rung { id: String },
    /// The removal sheet's own gesture: the books going out of the library,
    /// the shelves coming off the list, and the answer about the reading
    /// data.
    Removal {
        purge: Vec<String>,
        shelves: Vec<String>,
        data: ReadingData,
    },
}

/// One answer's button. Built at the raise rather than at the click, because
/// the sheet's own wording is a fact about the gesture and not something the
/// view recomputes.
#[derive(Clone, PartialEq)]
pub struct CopyOption {
    pub label: String,
    pub title: String,
    pub answer: CopyAnswer,
    pub primary: bool,
}

/// The reader's answer: buy the copies, finish the gesture without them, or
/// leave everything as it is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CopyAnswer {
    Copy,
    WithoutCopies,
    Cancel,
}

/// The question, ready for the sheet: what it is about, what it costs, and the answers.
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
    /// A book move: the rows the gate screened read in place, and the ground they are leaving.
    pub(super) fn of_rows(state: AppState, ids: &[String], hand: RowMove) -> Option<Self> {
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

    /// Shelves off the seat their folder's tree names: the level leaves the
    /// tree, and everything read in place under it leaves the ground with it.
    pub(super) fn of_shelf(
        state: AppState,
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
            // Offered only for a drop inside the mover's FAMILY: anywhere else
            // the copy is the only honest answer, because there is no tree to
            // put the shelf back into.
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
                format!("Moving it out stores its {cost}; the copy lands as “{}”.", copies[0]),
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

    /// A rung of a read-at-place tree coming apart: the level's own books
    /// leave the ground that made them, so they become the library's own
    /// before it goes.
    pub(super) fn of_apart(state: AppState, shelf_id: &str) -> Option<Self> {
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

    /// A shelf coming off the list with books read in place on it: the same
    /// copy, and the one removal that changes nothing — the folder's next
    /// import makes the level again.
    pub(super) fn of_removal(
        state: AppState,
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

/// A copy takes the level's next free name, so the folder's own name stays free for the original.
pub(super) fn free_name(name: &str, promised: &mut HashSet<String>) -> String {
    let free = library_core::book::duplicate_title(name, promised);
    promised.insert(free.clone());
    free
}

/// The books read in place that a rung takes with it when its ground goes:
/// the folder placed them, their own file answers to this rung, and they
/// stand inside it. A book answering to a level that stays — a seat the tree
/// still names — is not one of them, and neither is a book the library
/// already stores.
pub(super) fn books_the_rung_takes(state: AppState, shelf_id: &str) -> Vec<String> {
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

/// Every book the named shelves take with them, and none the removal is
/// taking anyway: a book going out of the library is not a book to copy
/// first.
fn shelf_books(state: AppState, shelves: &[String], purge: &[String]) -> Vec<String> {
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

/// The folder whose ground these books leave: the first that placed one of
/// them, and the one the sheet names.
fn ground_of(state: AppState, ids: &[String]) -> Option<String> {
    let books = state.library.books.get_untracked();
    let folders = state.library.folders.get_untracked();
    ids.iter().find_map(|id| {
        let fp = find_row(&books, id).and_then(|row| row.book()).map(|b| b.fp)?;
        let folder = folders
            .iter()
            .find(|f| f.mode().reads_in_place() && f.placed.contains(&fp))?;
        Some(folder_label(&folder.root))
    })
}

/// The gate every hand-move rides: a row that reads in place and is leaving
/// the ground that made it becomes the library's own stored copy, and a copy
/// is a question. `true` means the move waits on the sheet.
pub(super) fn ask_move_copy(state: AppState, ids: &[String], to: &str, hand: RowMove) -> bool {
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

/// The shelf half of the move: one question per gesture, raised only for
/// the shelves the folder's tree names a seat for.
pub(super) fn ask_move_shelf(
    state: AppState,
    departing: Vec<String>,
    target: Option<String>,
    seam: Option<ShelfSeam>,
) {
    if let Some(ask) = CopyAsk::of_shelf(state, departing, target, seam) {
        raise(state, ask);
    }
}

/// The removal sheet's own door: a shelf coming off the list that reads
/// books in place buys their copies first, and a removal with nothing to
/// copy runs at once.
pub fn remove_entries(
    state: AppState,
    purge: Vec<String>,
    shelves: Vec<String>,
    data: ReadingData,
) {
    match CopyAsk::of_removal(state, &purge, &shelves, data) {
        Some(ask) => raise(state, ask),
        None => remove(state, &purge, &shelves, data),
    }
}

pub(super) fn raise(state: AppState, ask: CopyAsk) {
    state.library.copy_ask.raise(ask);
}

/// Nothing moves and nothing copies.
pub fn cancel_copy(state: AppState) {
    state.library.copy_ask.dismiss();
}

/// The sheet's own answer. One entry for every button, so the view holds no logic.
pub fn answer_copy(state: AppState, answer: CopyAnswer) {
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

/// The answer with no copies in it: a shelf with a way home takes it, a
/// removal runs as it always did, and a gesture with no way home stays where
/// the folder's tree put it.
fn finish_without(state: AppState, ask: CopyAsk) {
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

/// The copies run in a spawned task — a shelf of fifty books is fifty files
/// through the store — so the sheet is off the screen at once.
async fn copy_and_finish(state: AppState, ask: CopyAsk) {
    match ask.work {
        CopyWork::Rows { ids, hand } => {
            let converting = converting_rows(state, &ids, hand.to());
            let copies = depart(state, &converting).await;
            // A copy that fails costs that book its move and nothing else: it stays where it was.
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

/// A removal, whole: the books go out of the library, and the shelves come
/// off the list deepest first, because a shelf dissolved first is a shelf no
/// sweep reaches.
fn remove(state: AppState, purge: &[String], shelves: &[String], data: ReadingData) {
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
