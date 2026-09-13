//! The read-at-place gate in front of a folder import: the family questions
//! a pick of ground answers BEFORE any sheet or walk — already imported, a
//! continuation of the tree's own root, a fold back into the family — and the
//! [`RootPlan`] the walk then starts from. The stored arrival asks the family
//! nothing and goes straight to the level's own name question.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::folder::{self as folder_ops, rel_under, FolderOpts, WatchedFolder};
use library_core::governance::Governance;
use library_core::id;
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops, Shelf, ShelfKind};

use super::claim::{already_importing, claim_root, root_is_claimed, when_root_is_free};
use super::folder::run_folder;
use super::tasks::{finish_task, push_task, task_id};
use super::{rel_of, root_shelf_of, shelf_name, Asked};
use crate::services::library::conflict;
use crate::services::library::folder_label;
use crate::state::library::ImportTask;
use crate::state::AppState;
use crate::time::now_ms;

/// What the folder sheet's answer decided about the run's root, before the run.
///
/// The default is the run nobody asked about: mint the folder's root shelf
/// under the folder's own name. A collision at the level changes that, and
/// the change is a value rather than a branch at six call sites:
///
///   * `rename` — the *as new* answer's name: the root rung is minted under
///     the counter the sheet promised rather than under the folder's own;
///   * `into` — the *merge* answer's shelf: the folder's root rung IS the
///     shelf the level already held, and every file the walk finds at the
///     root files into it;
///   * `continuation` — the shelf an already-imported read-at-place re-pick
///     names: the tree's root shelf for its own root, the rung's shelf for a
///     rung of it. It is the shelf the run LIGHTS when the walk found
///     something — the reader picked that ground and that ground is what
///     answers — and the note a walk that found NOTHING new owes the reader
///     after the fact: the reconciliation ran, every book was already here,
///     and the shelf lights up when the note closes. A run that found
///     something answers with the landing and the card instead, and a rescan
///     never carries one.
///   * `fold` — the family tree and the rung key this run's folder goes back
///     to when the walk ends: `(tree id, rel)`. An import of a folder is the
///     reader wanting it BACK, and back is the rung its directory names in the
///     in-place tree that covers it — so the walk runs on the pick's own
///     ledger and the run's last act folds the standing shelf into the family
///     (`reclaim_rung`), the seat the disk names rather than the level the
///     pick would otherwise mint on. Set by the gate for a pick inside a
///     family whose rung is gone, and for a re-pick of a folder that is
///     itself standing outside its family; a rescan never carries one.
#[derive(Clone, Default)]
pub(crate) struct RootPlan {
    pub rename: Option<String>,
    pub into: Option<String>,
    pub continuation: Option<(String, String)>,
    pub fold: Option<(String, String)>,
}

/// Import a folder, with the options the sheet was filled in with. Returns
/// immediately: the dock owns the feedback from here on.
///
/// A folder whose NAME the root level already holds is a question before it
/// is an import — two shelves of one name on one level are two doors a reader
/// cannot tell apart, which is the shelf's own spelling of the collision the
/// book sheet asks about. The question goes to the folder sheet
/// (`crate::services::library::conflict`), and the run starts with the answer
/// it gave as its [`RootPlan`]. ALWAYS: a reader who picked a folder and
/// clicked Import asked for an answer, and a run that ends on "Imported 0
/// books" with no sheet in between is the silent nothing the book collision
/// used to be. A folder colliding with its OWN previous shelf asks too — the
/// continuation is a choice rather than a surprise.
///
/// The read-at-place gate in front of all of it belongs to the READ-AT-PLACE
/// arrival alone, and it is the family's answer rather than the level's
/// (`covered_shelf`, `library_core::shelf::family_for`). A linked shelf IS an
/// OS folder, so which tree the pick belongs to is a question about the
/// ground and not about where the reader happens to be standing:
///
///   * ground the family already holds — the tree's OWN root re-picked, or
///     any RUNG of a tree the library reads in place — cannot mint a second
///     instance, and is not a bare sentence either: it is a RECONCILIATION.
///     The covering tree's walk runs on the reader's own ask — new files join
///     the tree as linked books, the logs a removal or a departure wrote are
///     spent by their books coming back — on the TREE's ledger, where a rung
///     has no second door to mint, and the shelf the pick named is the shelf
///     that lights up. Only a walk that found NOTHING raises the note, and a
///     tree that is itself a member standing outside another goes back inside
///     it on the run's answer, because an import of a folder is the reader
///     wanting it home. A book removed inside a rung comes back on a re-pick
///     of the rung exactly as on a re-pick of the root: both are the reader
///     asking the tree for its books again;
///   * a rung the family's ledger names but no shelf wears — deleted, or
///     departed as a copy — imports BACK INTO THE FAMILY: the walk runs on
///     the pick's own ledger, and the run's last act folds the shelf it
///     minted onto the rung its directory names (`RootPlan::fold`), lit
///     where it stands again. Importing a subfolder of a tree is how a
///     deleted rung comes back, not how a second door is made;
///   * a tree whose root rung a hand took OUT — the shelf's own departure —
///     arrives with no shelf and no family slot to name, and simply walks:
///     the copy the departure made wears the level's counter name and left
///     the folder's own name free, so the run re-mints the tree on the seats
///     the disk names and lights what came back.
///
/// A STORED arrival asks the family nothing: copies are the library's own
/// second instance, unrelated to any tree, and the only question they have is
/// the name one at the level they land on — the ordinary sheet, whose answers
/// for a stored arrival are the level's own three (go and look at the shelf
/// that is here, replace it, or a shelf of the next free name). The sheet
/// still withholds *as new* from a read-at-place arrival of a DIFFERENT
/// folder's name, because as new of a linked folder is exactly the second
/// instance the gate exists to prevent.
/// `track` is the sheet's watch answer for THIS ground, and it is an `Option`
/// because the two cases are not two settings of one thing. `None` is the
/// ordinary import: the switch wrote the options and the folder's own root rung
/// takes whatever the sheet said. `Some((tree, rung, on))` is a ground an
/// existing read-at-place tree already covers, where the decision belongs at that
/// rung of THAT tree rather than at a fresh folder's root — which is what lets a
/// subfolder be tracked differently from the import it stands inside, and what
/// lets a switch turned off on watched ground be an answer rather than a control
/// the reader was not allowed to touch.
pub fn import_folder(
    state: AppState,
    root: String,
    opts: FolderOpts,
    track: Option<(String, String, bool)>,
) {
    if let Some((tree_id, rung, on)) = &track {
        set_rung_tracking(state, tree_id, rung, *on);
    }
    if opts.in_place {
        if let Some(covered) = covered_shelf(state, &root) {
            // Ground a family tree already holds — its own root re-picked, or
            // a rung of it — is one reconciliation on the covering tree: the
            // walk runs on the reader's ask, on the TREE's ledger and root, so
            // a rung cannot mint a second instance of itself, new files join
            // the tree, and the books a removal logged come back wherever in
            // the tree they stood. The continuation names the shelf the pick
            // meant: it is the light when the walk found something and the
            // note's subject when it did not. And a tree standing outside a
            // family that could hold it goes home on the run's answer — the
            // same fold a subfolder pick gets from the gate below, promised
            // here rather than asked, because the reader just said which
            // folder they meant.
            let folders = state.library.folders.get_untracked();
            let fold = folders
                .iter()
                .find(|f| f.root == covered.tree_root)
                .and_then(|f| {
                    let shelves = state.library.shelves.get_untracked();
                    shelves_ops::family_for(&folders, &shelves, &f.root)
                });
            proceed_folder(
                state,
                covered.tree_root,
                opts,
                RootPlan {
                    continuation: Some((covered.shelf_id, covered.shelf_name)),
                    fold,
                    ..Default::default()
                },
            );
            return;
        }
        // Not covered — but the ground may still be a family's: the rung the
        // pick names was deleted, or departed as a copy, and the ledger that
        // covers the ground still stands. An import of a folder is the reader
        // wanting it BACK, and back is the rung its directory names in the
        // tree that covers it rather than a second shelf at the top of the
        // library: the walk runs on the pick's own ledger — where the
        // departed books' logs are, to be spent — and the run's last act
        // folds the shelf into the family.
        let fold = {
            let folders = state.library.folders.get_untracked();
            let shelves = state.library.shelves.get_untracked();
            shelves_ops::family_for(&folders, &shelves, &root)
        };
        if fold.is_some() {
            proceed_folder(
                state,
                root,
                opts,
                RootPlan {
                    fold,
                    ..Default::default()
                },
            );
            return;
        }
    }
    let incoming = folder_label(&root);
    let shelves = state.library.shelves.get_untracked();
    if let Some(existing_id) = library_core::conflict::collide_shelf(&shelves, None, &incoming) {
        // The folder's own root shelf, when a previous run minted one: the
        // `shelf_map`'s root rung is the whole of that memory.
        let own = state.library.folders.with_untracked(|folders| {
            folders
                .iter()
                .find(|f| f.root == root)
                .and_then(|f| f.shelf_map.get("").cloned())
                == Some(existing_id.clone())
        });
        let existing_name = shelves_ops::find(&shelves, &existing_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| incoming.clone());
        conflict::raise_shelf(
            state,
            conflict::ShelfConflictAsk {
                incoming_name: incoming,
                existing_id,
                existing_name,
                root,
                opts,
                own,
            },
        );
        return;
    }
    proceed_folder(state, root, opts, RootPlan::default());
}

/// The standing shelf an in-place tree already holds for `root`, if any: the
/// shelf the pick meant, and the tree whose ground answers for it.
///
/// A value rather than a tuple at the call site: the gate's covered branch
/// reconciles on `tree_root` and lights `shelf_id`, and a third string beside
/// those two is a second guessing about which is which.
pub(super) struct Covered {
    /// The shelf the pick meant: the tree's root shelf for its own root, the
    /// rung's shelf for a rung inside it.
    pub shelf_id: String,
    pub shelf_name: String,
    /// The root of the tree that covers the ground — the walk the re-pick
    /// owes runs on THIS ledger, whichever rung of it the reader picked.
    pub tree_root: String,
    /// The rung this ground names in that tree — `""` when the ground IS the
    /// tree's root. What makes a tracking decision about this ground a decision
    /// about this ground rather than about the whole import: the sheet's switch
    /// writes this rung, and a subfolder can then say something different from
    /// the tree it stands in.
    pub rel: String,
}

/// Whether the ground the sheet is about to import is tracked, and by which tree
/// at which rung — the three facts the watch switch needs to be the reader's own
/// question rather than a lock.
///
/// `None` when no read-at-place tree covers the ground, which is the ordinary
/// case: nothing is tracking it, so the switch simply writes the folder's own
/// root when the import lands. `Some` names the tree and the rung, so a switch
/// turned OFF on ground an ancestor tree tracks becomes an explicit decision at
/// THAT rung instead of a disabled control and a sentence telling the reader to
/// go and right-click something else.
pub fn ground_tracking(state: AppState, root: &str) -> Option<(String, String, bool)> {
    let covered = covered_shelf(state, root)?;
    let on = state.library.folders.with_untracked(|folders| {
        folder_ops::find(folders, &covered.tree_root)
            .map(|f| f.tracks_rung(&covered.rel))
            .unwrap_or(false)
    });
    Some((covered.tree_root, covered.rel, on))
}

/// Write one rung's tracking decision, on the tree that owns it.
///
/// The sheet's switch is the reader's answer about the ground they picked, and
/// the ground belongs to a rung of an existing tree whenever one covers it — so
/// the write is at that rung rather than at a fresh folder's root. Turning it ON
/// owes the quiet walk the shelf's own menu owes (`set_shelf_watch`), because a
/// reader who just asked the library to look at this ground should see the file
/// that arrived while nobody was watching, now rather than at the next focus.
/// It is the rescan's own walk, so the tombstones a removal wrote still hold.
fn set_rung_tracking(state: AppState, folder_id: &str, rung: &str, on: bool) {
    let mut changed = false;
    state.library.folders.update(|folders| {
        if let Some(folder) = folder_ops::find_mut(folders, folder_id) {
            folder.set_tracking(rung, on);
            changed = true;
        }
    });
    if !changed {
        return;
    }
    crate::storage::persist_library(state.library);
    if on {
        let Some((root, opts)) = state.library.folders.with_untracked(|folders| {
            folder_ops::find(folders, folder_id).map(|f| (f.root.clone(), f.opts.clone()))
        }) else {
            return;
        };
        super::verify::walk_one(state, root, opts);
    }
}

/// [`covered_of`] over the live lists.
pub(super) fn covered_shelf(state: AppState, root: &str) -> Option<Covered> {
    let folders = state.library.folders.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    covered_of(&folders, &shelves, root)
}

/// The standing shelf an in-place tree already holds for `root`, if any:
/// `root` IS a folder the library reads in place, or a subfolder inside one.
/// A folder's own tree answers for it before a tree it stands inside — the
/// empty rung wins — because the folder's own root shelf is the door the
/// reader meant.
///
/// The gate answers only for READ-AT-PLACE trees: their shelves are the OS
/// folders themselves, so a second import of the same ground is at best a
/// no-op and at worst a duplicate of every book on it. Stored trees are
/// never covered — a stored import is the library's own copy, and whether to
/// make another is the reader's call, asked through the ordinary name
/// question.
///
/// Pure over the two lists so a host test can name the case it is asserting:
/// the covered answer is what turns a re-pick into a reconciliation instead
/// of a second tree, and the turning has to be a table rather than a re-import
/// of a real folder read back through the note it raised.
pub(super) fn covered_of(
    folders: &[WatchedFolder],
    shelves: &[Shelf],
    root: &str,
) -> Option<Covered> {
    // The resolver owns the walk and the empty-rung precedence; this is the
    // gate's own value built from its answer. Both lookups below are guaranteed
    // to land — `covering` only names a shelf it found standing and a folder it
    // read out of `folders` — so the `?` is a shape, not a second rule.
    let coverage = Governance::new(folders, shelves).covering(root)?;
    let shelf = shelves_ops::find(shelves, &coverage.shelf_id)?;
    let folder = folder_ops::find(folders, &coverage.folder_id)?;
    Some(Covered {
        shelf_name: shelf.name.clone(),
        tree_root: folder.root.clone(),
        rel: coverage.rel,
        shelf_id: coverage.shelf_id,
    })
}

/// A member of this tree that is standing outside it.
///
/// The shape this answers, and it is one the reader makes rather than a bug:
/// a rung of a watched tree is removed, which cuts the folder's pointer to it
/// and leaves the folder watching; that same subfolder is then imported on its
/// own, which is ground no standing shelf covers, so it becomes a watched
/// folder of its own with a shelf wherever the import put it — usually the
/// top level. A re-import of the OUTER tree then finds every book already
/// standing and has nothing to say, while the shelf they are standing on is
/// not one of its own.
///
/// So the question is asked of the walk's own findings rather than of the shelf
/// list alone: another in-place folder whose root is a subfolder of this one,
/// whose root shelf still stands, which this walk actually found files under,
/// and which is not already hanging inside this tree — a shelf the reader
/// carried in by hand is where the reader put it and is none of this run's
/// business. The shallowest such member answers, because a deeper one hangs
/// under it and comes back with it.
///
/// An explicit run FOLDS the member it finds — an import is an ask, and a
/// member outside its family is an ask answered (`run_fold`) — rather than
/// asking a second question about it.
///
/// Answers the nested folder's id, the rung key its root directory names in
/// this tree, and the shelf's own id and name — the facts the fold runs on
/// and the report speaks.
pub(super) fn displaced_member(
    state: AppState,
    folder: &WatchedFolder,
    found: &[FoundFile],
) -> Option<DisplacedMember> {
    let folders = state.library.folders.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    let mut best: Option<(usize, DisplacedMember)> = None;
    for other in folders.iter() {
        if other.id == folder.id || !other.opts.in_place {
            continue;
        }
        let Some(rel) = rel_under(&other.root, &folder.root).filter(|rel| !rel.is_empty()) else {
            continue;
        };
        if !found
            .iter()
            .any(|file| rel_under(&file.path, &other.root).is_some())
        {
            continue;
        }
        // The folder's own root shelf, by its map first and by its kind second:
        // the map is what its walk files onto, and a map that lost the pointer
        // still leaves a shelf the folder owns. A folder with neither has no
        // standing shelf to ask about, and is not this run's question — it is
        // the next folder's turn rather than the end of the walk.
        let Some(shelf_id) = other
            .shelf_map
            .get("")
            .filter(|id| shelves.iter().any(|s| &s.id == *id))
            .cloned()
            .or_else(|| root_shelf_of(&shelves, &other.id))
        else {
            continue;
        };
        let Some(shelf) = shelves_ops::find(&shelves, &shelf_id) else {
            continue;
        };
        if shelf.kind.folder_id() == Some(folder.id.as_str())
            || hangs_inside(&shelves, &shelf_id, &folder.id)
        {
            continue;
        }
        let depth = rel.matches('/').count();
        if best.as_ref().is_none_or(|(seen, _)| depth < *seen) {
            best = Some((
                depth,
                DisplacedMember {
                    folder_id: other.id.clone(),
                    rel,
                    shelf_id,
                    shelf_name: shelf.name.clone(),
                },
            ));
        }
    }
    best.map(|(_, member)| member)
}

/// The facts a displaced member's note speaks and its answer writes.
pub(super) struct DisplacedMember {
    pub(super) folder_id: String,
    pub(super) rel: String,
    pub(super) shelf_id: String,
    pub(super) shelf_name: String,
}

/// Whether a shelf hangs inside a folder's tree: any ancestor of it is a shelf
/// that folder owns. Bounded by the list rather than by the walk finding its
/// own tail, for the reason `shelf::can_nest` bounds itself — a blob that
/// already carries a cycle answers "no" rather than spinning.
fn hangs_inside(shelves: &[Shelf], shelf_id: &str, folder_id: &str) -> bool {
    let mut current = shelves_ops::find(shelves, shelf_id).and_then(|s| s.parent.clone());
    for _ in 0..=shelves.len() {
        let Some(id) = current else {
            return false;
        };
        let Some(parent) = shelves_ops::find(shelves, &id) else {
            return false;
        };
        if parent.kind.folder_id() == Some(folder_id) {
            return true;
        }
        current = parent.parent.clone();
    }
    false
}

/// Put a displaced member back on the rung its directory names, and fold the
/// folder that was reading it into the tree that contains it.
///
/// The move the note's second answer owes, and the whole of it: one ground is
/// read by one folder from here on. Three writes, in the order that keeps them
/// honest —
///
///   * the tree's OWN chain down to the rung above the returning one, minted
///     with the walk's own arithmetic, because a reader who removed the rung
///     may have removed the one above it too and a shelf hanging on nothing
///     renders nowhere;
///   * the nested folder's shelves, each taking the tree's `rel` for the rung
///     it stands on — its root becomes the rung itself and its own subfolders
///     become rungs under that, so the disk's tree and the shelf's tree agree
///     again and the next re-hang has nothing to undo;
///   * the nested folder's ledger into the tree's, and the folder itself out.
///     Its `placed` set and its removals go with the ground: a fingerprint the
///     tree did not know it placed is a book its next rescan adds again, and a
///     removal it does not hold is a book its next rescan resurrects. Retiring
///     the folder without them would trade a duplicate shelf for a duplicated
///     library.
///
/// A shelf the reader hand-placed keeps its place through the move and is
/// marked as the reader's, which is what tells the next re-hang to pass it by:
/// the disk names its rung, and the hand still beats the disk. A nesting the
/// graph refuses — a blob that already carries a cycle — leaves the shelf where
/// it hangs for the same reason. A tree being walked right now refuses the move
/// outright: its run holds a clone of the ledger and writes it back whole, so a
/// fold made underneath it would be a fold that never happened.
pub(crate) fn reclaim_rung(
    state: AppState,
    tree_id: &str,
    gone_id: &str,
    rel: &str,
    shelf_id: &str,
) -> bool {
    let now = now_ms();
    let mut minted: Vec<Shelf> = Vec::new();
    let Some((mut tree, gone)) = state.library.folders.with_untracked(|folders| {
        let tree = folder_ops::find(folders, tree_id)?.clone();
        let gone = folder_ops::find(folders, gone_id)?.clone();
        Some((tree, gone))
    }) else {
        return false;
    };
    // Every shelf the nested folder owns, with the rung key it takes in the
    // tree: its root IS the rung, and a subfolder of it hangs under that. Read
    // before anything is written, because the answer is about the shelves that
    // are standing and not about the ones this move is going to mint.
    let rungs: Vec<(String, String)> = state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .filter(|s| s.kind.folder_id() == Some(gone_id))
            .filter_map(|s| {
                let ShelfKind::Folder { rel: own, .. } = &s.kind else {
                    return None;
                };
                let own = own.clone().unwrap_or_default();
                Some((
                    s.id.clone(),
                    if own.is_empty() {
                        rel.to_string()
                    } else {
                        format!("{rel}/{own}")
                    },
                ))
            })
            .collect()
    });
    // The shelf the note named is the one the answer is about: a shelf that
    // went while the note was up is an answer with nothing to move, and the
    // folder stays the folder it was rather than being folded away for nothing.
    // So does a tree that is being walked right now — its run holds a clone of
    // the ledger and writes it back whole at the end, which would drop
    // everything this fold put in it.
    if !rungs.iter().any(|(id, _)| id == shelf_id) || root_is_claimed(&tree.root) {
        return false;
    }
    // The tree's own rung above the returning one, minted whole: `key_chain`
    // walks from the tree's root shelf down, reusing the rungs the map holds
    // and reporting the ones it has to make.
    let root = tree.root.clone();
    let parent = tree.shelf_chain_for(
        library_core::folder::parent_key(rel).unwrap_or(""),
        |_| id::next_shelf_id(now),
        |rung| shelf_name(rung, &root),
        |rung, id, name, parent| {
            minted.push(Shelf {
                id: id.to_string(),
                name,
                kind: ShelfKind::Folder {
                    folder_id: tree_id.to_string(),
                    rel: rel_of(rung),
                },
                books: Vec::new(),
                parent,
                manual_parent: false,
            });
        },
    );
    for (id, key) in &rungs {
        tree.shelf_map.insert(key.clone(), id.clone());
    }
    // The ledger follows the ground.
    tree.placed.extend(gone.placed.iter().copied());
    for stone in gone.ignored.iter() {
        if !tree.is_ignored(&stone.fp) {
            tree.ignored.push(stone.clone());
        }
    }
    tree.scanned_ms = tree.scanned_ms.max(gone.scanned_ms);

    state.library.shelves.update(|shelves| {
        for shelf in minted {
            if !shelves.iter().any(|s| s.id == shelf.id) {
                shelves.push(shelf);
            }
        }
        let nestable = shelves_ops::can_nest(shelves, shelf_id, &parent);
        for (id, key) in &rungs {
            let Some(shelf) = shelves_ops::find_mut(shelves, id) else {
                continue;
            };
            shelf.kind = ShelfKind::Folder {
                folder_id: tree_id.to_string(),
                rel: rel_of(key),
            };
            if id != shelf_id {
                continue;
            }
            // The returning rung takes the place the disk names, unless the
            // reader's hand put it somewhere the graph will not undo.
            if nestable {
                shelf.parent = Some(parent.clone());
                shelf.manual_parent = false;
            } else {
                shelf.manual_parent = true;
            }
        }
    });
    state.library.folders.update(|folders| {
        folders.retain(|f| f.id != gone_id);
        match folders.iter().position(|f| f.id == tree_id) {
            Some(at) => folders[at] = tree,
            None => folders.push(tree),
        }
    });
    crate::storage::persist_library(state.library);
    true
}

/// Put the run in motion — the half of [`import_folder`] that is the same
/// whatever the folder sheet decided, and the half its answers call directly.
///
/// The reader's card goes up here, on the click that asked. The walk starts
/// here too, unless a rescan is walking this very root, in which case it starts
/// on that rescan's release — one walk per root, and an ask that outranks it.
pub(crate) fn proceed_folder(
    state: AppState,
    root: String,
    opts: FolderOpts,
    plan: RootPlan,
) {
    // A folder already being imported is an import already answering this ask:
    // its card is on the dock and its walk is the same tree. Racing it would
    // clobber its ledger write, so the second ask says so instead. A RESCAN of
    // the same tree is not a second ask and is not refused — an ask outranks it,
    // and the run waits out the walk the app started for itself rather than
    // losing the one that lifts a tombstone. `when_root_is_free` owns that rule.
    let task = task_id();
    let card = task.clone();
    let walking = root.clone();
    if !when_root_is_free(&root, move || {
        start_folder_run(state, walking, opts, plan, card)
    }) {
        already_importing(state, &root);
        return;
    }
    // The card goes up whether the walk started now or is waiting for a rescan
    // to finish: the reader clicked Import, and a dock with nothing on it while
    // a folder is being walked is a dock that says the click did nothing.
    push_task(state, ImportTask::new(task, folder_label(&root)));
}

/// Claim the root and start the walk. One shape for the two starts an import
/// has — the click that found the root free, and the release of the rescan that
/// was walking it when the click arrived — because the two owe the same run and
/// the same card, and a second spelling is a second place to forget the claim.
fn start_folder_run(
    state: AppState,
    root: String,
    opts: FolderOpts,
    plan: RootPlan,
    task: String,
) {
    let Some(claim) = claim_root(&root, Asked::Explicitly) else {
        // The root went to a run of its own between a rescan's release and this
        // start, which the single thread leaves no room for — but the card is
        // already up, so the card is closed rather than left counting a walk
        // that never started.
        finish_task(state, &task, 0, 0);
        return;
    };
    spawn_local(async move {
        // Held for the whole run: the drop is the release, on every exit path,
        // and the release is what hands the root to an ask queued behind it.
        let _claim = claim;
        run_folder(state, task, root, opts, Asked::Explicitly, plan).await;
    });
}

/// The fold an explicit run owes at its end, and the shelf it seated.
///
/// The plan's own fold first: the run walked the picked folder's ledger, and
/// its root shelf is the member going home — an import of a folder a family
/// tree covers is the reader wanting it back on the rung its directory names,
/// which is `reclaim_rung`'s own arithmetic and not a second one. With no
/// fold planned, the run asks whether a member of the tree it just walked is
/// standing OUTSIDE it — the shallowest, because a deeper one hangs under it
/// and comes back with it — and puts that home: an import is an ask, and a
/// member outside its family is an ask answered rather than a second question.
///
/// `None` when nothing folded: a rescan, which never asks and never moves; a
/// tree with no member outside it; or a fold the guards refused — the family
/// being walked by a run of its own, or the shelf gone while this one ran. A
/// refused fold is not news: the member stands where it stood, and the next
/// explicit import answers again.
pub(super) fn run_fold(
    state: AppState,
    plan: &RootPlan,
    folder: &WatchedFolder,
    root_rung: Option<&str>,
    found: &[FoundFile],
) -> Option<(String, String)> {
    if let Some((tree, rel)) = &plan.fold {
        let rung = root_rung?;
        reclaim_rung(state, tree, &folder.id, rel, rung)
            .then(|| (rung.to_string(), state.library.shelf_name(rung)))
    } else {
        let member = displaced_member(state, folder, found)?;
        reclaim_rung(
            state,
            &folder.id,
            &member.folder_id,
            &member.rel,
            &member.shelf_id,
        )
        .then(|| (member.shelf_id.clone(), member.shelf_name.clone()))
    }
}
