//! Shelf membership: the ids a shelf holds, and the edits over them.

use super::{ALL_SHELF, Shelf};

/// Put `id` on a member list at `index`, or move it there.
pub fn place(members: &mut Vec<String>, id: &str, index: Option<usize>) {
    members.retain(|m| m != id);
    let at = index.unwrap_or(members.len()).min(members.len());
    members.insert(at, id.to_string());
}

/// The ids one level holds, in order.
pub fn members_of<'a>(
    rows: &'a [crate::book::Row],
    shelves: &'a [Shelf],
    shelf_id: &str,
) -> Vec<&'a str> {
    if let Some(shelf) = shelves.iter().find(|s| s.id == shelf_id) {
        return shelf.books.iter().map(String::as_str).collect();
    }
    if shelf_id != ALL_SHELF {
        return Vec::new();
    }
    // One pass over the memberships.
    let filed: std::collections::HashSet<&str> = shelves
        .iter()
        .flat_map(|s| s.books.iter().map(String::as_str))
        .collect();
    rows.iter()
        .map(crate::book::Row::id)
        .filter(|id| !filed.contains(id))
        .collect()
}

/// Put `id` on a shelf unless it is already there.
pub fn shelf_add(shelf: &mut Shelf, book_id: &str) {
    if !shelf.books.iter().any(|member| member == book_id) {
        shelf.books.push(book_id.to_string());
    }
}

pub fn forget(members: &mut Vec<String>, id: &str) -> bool {
    let before = members.len();
    members.retain(|m| m != id);
    members.len() != before
}

/// Drop a book from every shelf at once.
pub fn forget_everywhere(shelves: &mut [Shelf], book_id: &str) {
    for shelf in shelves.iter_mut() {
        forget(&mut shelf.books, book_id);
    }
}

pub fn containing<'a>(shelves: &'a [Shelf], book_id: &str) -> Vec<&'a Shelf> {
    shelves
        .iter()
        .filter(|s| s.books.iter().any(|m| m == book_id))
        .collect()
}
