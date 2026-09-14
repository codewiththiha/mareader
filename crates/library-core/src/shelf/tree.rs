//! The shelf tree: which shelves hang under which, and the moves that
//! re-hang them — the cycle-checked nest, the subtree a departing shelf
//! takes with it, and the pass that puts a watched folder's rungs back on
//! the seats their directories name.

use super::{find, Shelf, ShelfKind};

/// The shelves filed directly inside `parent_id`; `None` asks for the root level.
/// Direct children only: a level is a page, and flattening the subtree would show
/// shelves the reader has not opened.
pub fn children_of<'a>(shelves: &'a [Shelf], parent_id: Option<&str>) -> Vec<&'a Shelf> {
    shelves
        .iter()
        .filter(|s| s.parent.as_deref() == parent_id)
        .collect()
}

/// The chain above `id`, root first and excluding `id` — what a breadcrumb walks.
/// The walk stops on a shelf it has already seen: a breadcrumb that looped would
/// hang the render rather than show one crumb too many.
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

/// Whether `folder_id` may be filed inside `target_id`. Both refusals are about
/// the same failure, a shelf inside itself: the drop on itself, and the drop into
/// one of its own descendants.
pub fn can_nest(shelves: &[Shelf], folder_id: &str, target_id: &str) -> bool {
    if folder_id == target_id {
        return false;
    }
    let mut current = Some(target_id.to_string());
    // Bounded by the list rather than by the walk finding its own tail: a blob that
    // already carries a cycle would otherwise spin here forever.
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

/// File `folder_id` inside `parent`, or at the root when `parent` is `None`. True
/// when the shelf was found and the graph allows the move; a refused drop leaves
/// the list exactly as it was, so the caller can answer it by doing nothing.
///
/// A watched shelf keeps every fact it has except its place, and the mark written
/// here is what stops the next re-hang from undoing a hand the disk disagrees
/// with.
pub fn reparent(shelves: &mut [Shelf], folder_id: &str, parent: Option<&str>) -> bool {
    if let Some(target) = parent
        && !can_nest(shelves, folder_id, target)
    {
        return false;
    }
    // The seat is read before the write borrow: which place the disk names is a fact about the list as it stands.
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

/// The folder's rungs: every shelf of one folder, from the `rel` key to the shelf
/// id wearing it — one map for the seat question, the re-hang and the shape a
/// re-import moves a tree by, so the callers cannot drift.
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

/// The parent the folder's own SHELVES name for a folder shelf: the rung above
/// its `rel`, or the library's root for a top-level rung. `None` for a shelf that
/// is no folder's rung, where there is no disk answer to compare against.
fn folder_seat(shelves: &[Shelf], shelf_id: &str) -> Option<Option<String>> {
    let shelf = find(shelves, shelf_id)?;
    let ShelfKind::Folder { folder_id, rel } = &shelf.kind else {
        return None;
    };
    let key = rel.clone().unwrap_or_default();
    let rungs = rungs_of(shelves, folder_id);
    Some(
        crate::folder::parent_key(&key)
            .and_then(|rung| rungs.get(rung))
            .map(|id| id.to_string()),
    )
}


/// Every shelf below any of `roots`, at any depth, in no particular order,
/// without repeats and without the roots themselves. Walked with an explicit stack
/// and a seen-set rather than recursively: this reads a list that can be caught
/// between two writes, and a recursion over a graph with a loop in it is a stack
/// overflow.
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

/// Move a shelf's children up to the level it was on: a child left pointing at a
/// parent that is gone renders on no level at all.
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

/// The moves a watched folder's rescan owes its own shelves: every shelf the
/// folder owns that is not hand-moved, when the rung its `rel` names resolves to
/// a different parent than the one it hangs on. A folder card cut from a watched
/// tree is a VIEW of that tree, so the disk's shape wins for the shelves the
/// folder owns.
pub fn rehang_moves(shelves: &[Shelf], folder_id: &str) -> Vec<(String, Option<String>)> {
    // The folder's rungs: the one map the seat question reads too, so the re-hang and the hand's mark cannot disagree.
    let rungs = rungs_of(shelves, folder_id);
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
        let key = rel.clone().unwrap_or_default();
        let want = crate::folder::parent_key(&key)
            .and_then(|rung| rungs.get(rung).copied())
            .filter(|w| *w != shelf.id.as_str())
            .map(str::to_string);
        if shelf.parent != want {
            moved.push((shelf.id.clone(), want));
        }
    }
    moved
}
