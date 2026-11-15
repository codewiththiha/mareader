//! Books, shelves and links picked on the page, to act on together.

use std::collections::BTreeSet;

use crate::id::BookId;

/// A card picked on the page: the library's own item.
pub use crate::arrange::Item as Pick;

/// Picking asked for by clicks, keys and the selection bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickAction {
    /// In or out; it anchors a later Shift-click.
    Toggle(Pick),
    /// Added, keeping the anchor: a Shift-click's range, or All.
    Add(Vec<Pick>),
    /// These and only these: Select all.
    Only(Vec<Pick>),
    /// Ends selecting: Done, Escape or a click on empty space.
    Clear,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picks {
    set: BTreeSet<Pick>,
    anchor: Option<Pick>,
    /// Selecting outlasts its last pick, until the reader is done.
    selecting: bool,
}

impl Picks {
    /// Whether the page is selecting: clicks pick and the bar shows.
    pub fn active(&self) -> bool {
        self.selecting
    }

    pub fn contains(&self, pick: Pick) -> bool {
        self.set.contains(&pick)
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Whether a Shift-click has somewhere to run from.
    pub fn has_anchor(&self) -> bool {
        self.anchor.is_some()
    }

    /// Every pick: shelves, then books, then links.
    pub fn items(&self) -> Vec<Pick> {
        self.set.iter().copied().collect()
    }

    pub fn books(&self) -> Vec<BookId> {
        let books = self.set.iter().filter_map(|p| match p {
            Pick::Book(id) => Some(*id),
            _ => None,
        });
        books.collect()
    }

    /// From the anchor to `to` in the order shown; else just `to`.
    pub fn range(&self, order: &[Pick], to: Pick) -> Vec<Pick> {
        let at = |p: Pick| order.iter().position(|&o| o == p);
        let (Some(from), Some(end)) = (self.anchor.and_then(at), at(to)) else {
            return vec![to];
        };
        let (lo, hi) = (from.min(end), from.max(end));
        order
            .get(lo..=hi)
            .map_or_else(|| vec![to], <[Pick]>::to_vec)
    }

    pub fn apply(&mut self, action: PickAction) {
        self.selecting = action != PickAction::Clear;
        match action {
            PickAction::Toggle(pick) => {
                if !self.set.remove(&pick) {
                    self.set.insert(pick);
                }
                self.anchor = Some(pick);
            }
            PickAction::Add(picks) => self.set.extend(picks),
            PickAction::Only(picks) => {
                self.set = picks.into_iter().collect();
                self.anchor = None;
            }
            PickAction::Clear => *self = Self::default(),
        }
    }

    /// Drops picks that no longer exist.
    pub fn retain(&mut self, keep: impl Fn(Pick) -> bool) {
        self.set.retain(|&p| keep(p));
        self.anchor = self.anchor.filter(|&p| keep(p));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_runs_from_the_anchor_either_way() {
        let order: Vec<Pick> = (1..=5).map(|n| Pick::Book(BookId(n))).collect();
        let mut picks = Picks::default();
        let b = |n| Pick::Book(BookId(n));
        assert_eq!(picks.range(&order, b(3)), [b(3)]);
        picks.apply(PickAction::Toggle(b(4)));
        assert_eq!(picks.range(&order, b(2)), [b(2), b(3), b(4)]);
        assert_eq!(picks.range(&order, b(5)), [b(4), b(5)]);
        picks.apply(PickAction::Add(vec![b(1), b(2)]));
        picks.apply(PickAction::Toggle(b(4)));
        assert_eq!(picks.books(), [BookId(1), BookId(2)]);
        picks.retain(|p| p != b(1));
        assert_eq!(picks.len(), 1);
        picks.apply(PickAction::Toggle(b(2)));
        assert!(picks.is_empty() && picks.active(), "still selecting");
        picks.apply(PickAction::Clear);
        assert!(!picks.active());
    }
}
