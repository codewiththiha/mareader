//! The shelf tree: which shelves hang under which, and the moves that
//! re-hang them — the cycle-checked nest, the subtree a departing shelf takes
//! with it, and the pass that puts a watched folder's rungs back on the seats
//! their directories name.

use super::{Shelf, ShelfKind, find};

/// The shelves filed directly inside `parent_id`; `None` asks for the root
/// level. Direct children only: a level is a page, and flattening the subtree
/// would show shelves the reader has not opened.
pub fn children_of<'a>(shelves: &'a [Shelf], parent_id: Option<&str>) -> Vec<&'a Shelf> {
    shelves
        .iter()
        .filter(|s| s.parent.as_deref() == parent_id)
        .collect()
}

/// The chain above `id`, root first, excluding `id` — what a breadcrumb
/// walks. Stops on a shelf it has already seen: a breadcrumb that looped
/// would hang the render.
pub fn ancestors<'a>(shelves: &'a [Shelf], id: &str) -> Vec<&'a Shelf> {
    let mut chain: Vec<&'a Shelf> = Vec::new();
    let mut next = find(shelves, id).and_then(|s| s.parent.as_deref());
    while let Some(parent_id) = next {
        if chain.iter().any(|seen| seen.id == parent_id) {
            break;
        }
        let Some(parent) = shelves.iter().find(|s| s.id == parent_id) else {
            break;
        };
        chain.push(parent);
        next = parent.parent.as_deref();
    }
    chain.reverse();
    chain
}

/// Whether `folder_id` may be filed inside `target_id`. Both refusals are the
/// same failure — a shelf inside itself: the drop on itself, and the drop
/// into one of its own descendants.
pub fn can_nest(shelves: &[Shelf], folder_id: &str, target_id: &str) -> bool {
    if folder_id == target_id {
        return false;
    }
    let mut current = Some(target_id.to_string());
    // Bounded by the list length: a blob that already carries a cycle would
    // otherwise spin here forever.
    for _ in 0..=shelves.len() {
        let Some(id) = current else {
            return true;
        };
        if id == folder_id {
            return false;
        }
        current = shelves
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.parent.clone());
    }
    false
}

/// File `folder_id` inside `parent`, or at the root when `parent` is `None`.
/// True when the shelf was found and the graph allows the move; a refused drop
/// leaves the list untouched, so the caller answers it by doing nothing.
///
/// Writes the `manual_parent` mark when the new place differs from the seat
/// the disk names — the mark that stops the next re-hang from undoing a hand
/// the disk disagrees with — and clears it when a hand puts the shelf back.
pub fn reparent(shelves: &mut [Shelf], folder_id: &str, parent: Option<&str>) -> bool {
    if let Some(target) = parent
        && !can_nest(shelves, folder_id, target)
    {
        return false;
    }
    // The seat is read before the write borrow: which place the disk names
    // is a fact about the list as it stands.
    let seat = folder_seat(shelves, folder_id);
    let Some(shelf) = shelves.iter_mut().find(|s| s.id == folder_id) else {
        return false;
    };
    if let Some(seat) = seat {
        shelf.manual_parent = seat.as_deref() != parent;
    }
    shelf.parent = parent.map(str::to_string);
    true
}

/// The folder's rungs: `rel` key to shelf id for every shelf of one folder.
/// One map for the seat question, the re-hang and the re-import shape, so the
/// callers cannot drift.
pub fn rungs_of<'a>(
    shelves: &'a [Shelf],
    folder_id: &str,
) -> std::collections::HashMap<String, &'a str> {
    shelves
        .iter()
        .filter_map(|s| match &s.kind {
            ShelfKind::Folder {
                folder_id: owner,
                rel,
            } if owner == folder_id => Some((rel.clone().unwrap_or_default(), s.id.as_str())),
            _ => None,
        })
        .collect()
}

/// The nearest rung standing above `key`. A level taken apart leaves
/// everything under it inside the tree — a key whose own rung is gone would
/// otherwise answer the library's top level, where the folder's next scan
/// cannot see the shelf. `None` only for the root rung or a tree whose rungs
/// have all gone.
pub fn rung_above(shelves: &[Shelf], folder_id: &str, key: &str) -> Option<String> {
    let rungs = rungs_of(shelves, folder_id);
    let mut above = crate::folder::parent_key(key)?;
    loop {
        if let Some(id) = rungs.get(above) {
            return Some(id.to_string());
        }
        above = crate::folder::parent_key(above)?;
    }
}

/// The parent the folder's own shelves name for a folder shelf: the nearest
/// rung still standing above its `rel`, or the library's root for a top-level
/// rung. `None` for a shelf that is no folder's rung — no disk answer to
/// compare against.
fn folder_seat(shelves: &[Shelf], shelf_id: &str) -> Option<Option<String>> {
    let shelf = find(shelves, shelf_id)?;
    let ShelfKind::Folder { folder_id, rel } = &shelf.kind else {
        return None;
    };
    let key = rel.clone().unwrap_or_default();
    Some(rung_above(shelves, folder_id, &key))
}

/// Every shelf below any of `roots`, at any depth, without repeats and
/// without the roots themselves. An explicit stack rather than recursion:
/// this reads a list that can be caught between two writes, and recursing
/// over a graph with a loop in it is a stack overflow.
pub fn subtree_ids(shelves: &[Shelf], roots: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<String> = roots.to_vec();
    while let Some(parent) = stack.pop() {
        for child in children_of(shelves, Some(parent.as_str())) {
            if roots.iter().any(|each| each == &child.id)
                || out.iter().any(|each| each == &child.id)
            {
                continue;
            }
            stack.push(child.id.clone());
            out.push(child.id.clone());
        }
    }
    out
}

/// Move a shelf's children up to the level it was on: a child left pointing
/// at a gone parent renders on no level at all.
pub fn lift_children(shelves: &mut [Shelf], folder_id: &str) {
    let inherited = shelves
        .iter()
        .find(|s| s.id == folder_id)
        .and_then(|s| s.parent.clone());
    for shelf in shelves.iter_mut() {
        if shelf.parent.as_deref() == Some(folder_id) {
            shelf.parent = inherited.clone();
        }
    }
}

/// The moves a watched folder's rescan owes its own shelves: every
/// non-hand-moved shelf the folder owns whose `rel` resolves to a different
/// parent than the one it hangs on. A folder card cut from a watched tree is
/// a view of that tree, so the disk's shape wins.
pub fn rehang_moves(shelves: &[Shelf], folder_id: &str) -> Vec<(String, Option<String>)> {
    let mut moved = Vec::new();
    for shelf in shelves.iter() {
        // The reader's placement wins over the disk's shape.
        if shelf.manual_parent {
            continue;
        }
        let ShelfKind::Folder {
            folder_id: owner,
            rel,
        } = &shelf.kind
        else {
            continue;
        };
        if owner != folder_id {
            continue;
        }
        let want = rung_above(shelves, folder_id, rel.as_deref().unwrap_or(""));
        if shelf.parent != want {
            moved.push((shelf.id.clone(), want));
        }
    }
    moved
}
