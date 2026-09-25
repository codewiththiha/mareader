//! What a drop means, decided from four facts: what is held, what is under
//! the pointer, which part of the target's box the pointer is on, and how long
//! it has been there.
//!
//! Pure on purpose — no DOM, no signals, no state — which makes the table
//! testable on the host rather than in a browser.

use super::target::DropTargetKind;
use crate::features::library::folder_card::THUMB_CAP;

/// Two, because a shelf of one is a shelf the view menu already makes. The
/// book under the pointer counts as the second, so the offer starts at the
/// first item held.
pub const FOLD_MIN_ITEMS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Top,
    Middle,
    Bottom,
}

impl Band {
    pub fn after(self) -> bool {
        self == Band::Bottom
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropEffect {
    InsertBefore {
        book_id: String,
        /// Named by the effect rather than re-derived at the commit: a nested
        /// tree row's container is not the open level.
        shelf: Option<String>,
        after: bool,
    },
    ShelfSibling {
        anchor_id: String,
        after: bool,
    },
    FileToShelf {
        shelf_id: String,
    },
    NestInto {
        folder_id: String,
    },
    CreateFolder {
        with_book_id: String,
    },
    /// Distinct from "nothing under the pointer" (a `None` effect), so a card
    /// can tell "not a target" from "a target that says no".
    Refused,
}

impl DropEffect {
    pub fn insert_at(&self) -> Option<(&str, bool)> {
        match self {
            DropEffect::InsertBefore { book_id, after, .. } => Some((book_id.as_str(), *after)),
            _ => None,
        }
    }

    pub fn sibling_at(&self) -> Option<(&str, bool)> {
        match self {
            DropEffect::ShelfSibling { anchor_id, after } => Some((anchor_id.as_str(), *after)),
            _ => None,
        }
    }

    pub fn nest_into(&self) -> Option<&str> {
        match self {
            DropEffect::NestInto { folder_id } => Some(folder_id.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldPreview {
    /// Capped at the plate's own cap: a preview showing more cells would
    /// preview a shape the library does not draw.
    pub filled: usize,
    pub with_book_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropQuery<'a> {
    pub held_books: usize,
    pub held_folders: usize,
    pub target_kind: DropTargetKind,
    pub target_id: &'a str,
    pub target_is_held: bool,
    pub can_nest: bool,
    /// The parent's `can_nest` answer: filing beside a shelf is filing into
    /// the level that holds it.
    pub can_sibling: bool,
    pub band: Band,
    pub target_shelf: Option<&'a str>,
    pub dwell_armed: bool,
}

impl DropQuery<'_> {
    pub fn held(&self) -> usize {
        self.held_books + self.held_folders
    }
}

/// Held items plus the target. A target that counted itself made a drag of
/// two onto one a shelf holding that member twice.
pub fn fold_items(query: &DropQuery<'_>) -> usize {
    query.held() + 1
}

pub fn drop_effect(query: DropQuery<'_>) -> DropEffect {
    match query.target_kind {
        DropTargetKind::Book => {
            let id = query.target_id;
            let shelf = query.target_shelf.map(str::to_string);
            // A book the pointer is already carrying is a position and never
            // a partner — including a single book dragged onto itself, which
            // lands where it started: the no-op it looks like.
            if query.target_is_held {
                return DropEffect::InsertBefore {
                    book_id: id.to_string(),
                    shelf,
                    after: false,
                };
            }
            // A row is a seam between books and a shelf is not a book: no
            // position to take and no fold to brew.
            if query.held_books == 0 {
                return DropEffect::Refused;
            }
            // The dwell is the whole difference, and not a refinement:
            // without it a reorder would be unreachable, because every card a
            // drag crossed would offer a new shelf.
            if query.dwell_armed && fold_items(&query) >= FOLD_MIN_ITEMS {
                return DropEffect::CreateFolder {
                    with_book_id: id.to_string(),
                };
            }
            DropEffect::InsertBefore {
                book_id: id.to_string(),
                shelf,
                after: query.band.after(),
            }
        }
        DropTargetKind::Folder => {
            let id = query.target_id;
            // A book is always welcome; a shelf is welcome unless filing it
            // here would put it inside itself — a folder no level renders and
            // a reader can never open again.
            if query.band == Band::Middle || query.held_books > 0 || query.held_folders == 0 {
                if query.held_folders > 0 && !query.can_nest {
                    return DropEffect::Refused;
                }
                return DropEffect::NestInto {
                    folder_id: id.to_string(),
                };
            }
            if query.target_is_held || !query.can_sibling {
                return DropEffect::Refused;
            }
            DropEffect::ShelfSibling {
                anchor_id: id.to_string(),
                after: query.band.after(),
            }
        }
        DropTargetKind::Shelf | DropTargetKind::Level => DropEffect::FileToShelf {
            shelf_id: query.target_id.to_string(),
        },
        DropTargetKind::Ellipsis => DropEffect::Refused,
    }
}

pub fn fold_preview(items: usize, with_book_id: &str) -> Option<FoldPreview> {
    (items >= FOLD_MIN_ITEMS).then(|| FoldPreview {
        filled: items.min(THUMB_CAP),
        with_book_id: with_book_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(kind: DropTargetKind, id: &str, books: usize, folders: usize) -> DropQuery<'_> {
        DropQuery {
            held_books: books,
            held_folders: folders,
            target_kind: kind,
            target_id: id,
            target_is_held: false,
            can_nest: true,
            can_sibling: false,
            band: Band::Middle,
            target_shelf: None,
            dwell_armed: false,
        }
    }

    fn rested(kind: DropTargetKind, id: &str, books: usize, folders: usize) -> DropQuery<'_> {
        DropQuery {
            dwell_armed: true,
            ..query(kind, id, books, folders)
        }
    }

    fn insert(id: &str, shelf: Option<&str>, after: bool) -> DropEffect {
        DropEffect::InsertBefore {
            book_id: id.to_string(),
            shelf: shelf.map(str::to_string),
            after,
        }
    }

    #[test]
    fn a_book_under_a_drag_is_where_the_held_items_land() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Book, "b2", 1, 0)),
            insert("b2", None, false)
        );
    }

    #[test]
    fn the_bottom_half_of_a_book_row_lands_the_hold_after_it() {
        assert_eq!(
            drop_effect(DropQuery {
                band: Band::Bottom,
                ..query(DropTargetKind::Book, "b2", 1, 0)
            }),
            insert("b2", None, true)
        );
        assert_eq!(
            drop_effect(DropQuery {
                band: Band::Top,
                ..query(DropTargetKind::Book, "b2", 1, 0)
            }),
            insert("b2", None, false),
            "the top half is the before it has always been"
        );
        assert_eq!(
            drop_effect(DropQuery {
                band: Band::Bottom,
                target_shelf: Some("s2"),
                ..query(DropTargetKind::Book, "b2", 1, 0)
            }),
            insert("b2", Some("s2"), true)
        );
    }

    #[test]
    fn a_hold_with_no_books_in_it_is_refused_by_a_book_row() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Book, "b2", 0, 1)),
            DropEffect::Refused
        );
        assert_eq!(
            drop_effect(DropQuery {
                target_shelf: Some("s2"),
                ..query(DropTargetKind::Book, "b2", 0, 2)
            }),
            DropEffect::Refused
        );
        assert_eq!(
            drop_effect(rested(DropTargetKind::Book, "b2", 0, 1)),
            DropEffect::Refused,
            "a dwell offers a fold to a hold with books in it, and nothing to this"
        );
        assert_eq!(
            drop_effect(DropQuery {
                target_shelf: Some("s2"),
                band: Band::Bottom,
                ..query(DropTargetKind::Book, "b2", 1, 1)
            }),
            insert("b2", Some("s2"), true)
        );
    }

    #[test]
    fn resting_over_an_unheld_book_folds_it_with_the_hold() {
        assert!(matches!(
            drop_effect(query(DropTargetKind::Book, "b2", 1, 0)),
            DropEffect::InsertBefore { .. }
        ));
        assert_eq!(
            drop_effect(rested(DropTargetKind::Book, "b2", 1, 0)),
            DropEffect::CreateFolder {
                with_book_id: "b2".to_string()
            }
        );
        assert_eq!(
            drop_effect(rested(DropTargetKind::Book, "b2", 3, 1)),
            DropEffect::CreateFolder {
                with_book_id: "b2".to_string()
            }
        );
    }

    #[test]
    fn a_book_the_pointer_is_carrying_is_a_position_and_never_a_partner() {
        let onto_itself = DropQuery {
            target_is_held: true,
            ..rested(DropTargetKind::Book, "b1", 1, 0)
        };
        assert_eq!(drop_effect(onto_itself), insert("b1", None, false));
        let onto_the_set = DropQuery {
            target_is_held: true,
            ..rested(DropTargetKind::Book, "b2", 3, 1)
        };
        assert!(matches!(
            drop_effect(onto_the_set),
            DropEffect::InsertBefore { .. }
        ));
        let unrested = DropQuery {
            target_is_held: true,
            ..query(DropTargetKind::Book, "b2", 3, 0)
        };
        assert!(matches!(
            drop_effect(unrested),
            DropEffect::InsertBefore { .. }
        ));
    }

    #[test]
    fn the_plate_counts_the_partner_once_and_stops_at_four_cells() {
        assert_eq!(fold_items(&query(DropTargetKind::Book, "b2", 1, 0)), 2);
        assert_eq!(fold_items(&query(DropTargetKind::Book, "b2", 2, 0)), 3);
        assert_eq!(fold_items(&query(DropTargetKind::Book, "b2", 0, 1)), 2);
        assert_eq!(fold_items(&query(DropTargetKind::Book, "b2", 3, 1)), 5);

        assert_eq!(fold_preview(1, "b2"), None);
        assert_eq!(
            fold_preview(2, "b2"),
            Some(FoldPreview {
                filled: 2,
                with_book_id: "b2".to_string()
            })
        );
        assert_eq!(fold_preview(3, "b2").unwrap().filled, 3);
        assert_eq!(fold_preview(4, "b2").unwrap().filled, THUMB_CAP);
        assert_eq!(fold_preview(9, "b2").unwrap().filled, THUMB_CAP);
    }

    #[test]
    fn the_middle_of_a_shelf_row_takes_the_hold_inside() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Folder, "f1", 0, 1)),
            DropEffect::NestInto {
                folder_id: "f1".to_string()
            }
        );
        assert_eq!(
            drop_effect(query(DropTargetKind::Folder, "f1", 2, 0)),
            DropEffect::NestInto {
                folder_id: "f1".to_string()
            }
        );
        let refused = DropQuery {
            can_nest: false,
            ..query(DropTargetKind::Folder, "f1", 0, 1)
        };
        assert_eq!(drop_effect(refused), DropEffect::Refused);
    }

    #[test]
    fn the_edges_of_a_shelf_row_reorder_siblings() {
        let sibling = |band: Band| DropQuery {
            band,
            can_sibling: true,
            ..query(DropTargetKind::Folder, "f1", 0, 1)
        };
        assert_eq!(
            drop_effect(sibling(Band::Top)),
            DropEffect::ShelfSibling {
                anchor_id: "f1".to_string(),
                after: false
            }
        );
        assert_eq!(
            drop_effect(sibling(Band::Bottom)),
            DropEffect::ShelfSibling {
                anchor_id: "f1".to_string(),
                after: true
            }
        );
    }

    #[test]
    fn books_on_a_shelf_rows_edge_still_go_inside_it() {
        for band in [Band::Top, Band::Bottom] {
            assert_eq!(
                drop_effect(DropQuery {
                    band,
                    can_sibling: true,
                    ..query(DropTargetKind::Folder, "f1", 2, 0)
                }),
                DropEffect::NestInto {
                    folder_id: "f1".to_string()
                }
            );
            assert!(matches!(
                drop_effect(DropQuery {
                    band,
                    can_sibling: true,
                    ..query(DropTargetKind::Folder, "f1", 1, 1)
                }),
                DropEffect::NestInto { .. }
            ));
        }
    }

    #[test]
    fn a_sibling_the_graph_refuses_is_refused() {
        let edge = DropQuery {
            band: Band::Top,
            can_sibling: false,
            ..query(DropTargetKind::Folder, "f1", 0, 1)
        };
        assert_eq!(drop_effect(edge), DropEffect::Refused);
        let onto_itself = DropQuery {
            band: Band::Bottom,
            can_sibling: true,
            target_is_held: true,
            ..query(DropTargetKind::Folder, "f1", 0, 1)
        };
        assert_eq!(drop_effect(onto_itself), DropEffect::Refused);
    }

    #[test]
    fn a_nesting_is_never_refused_by_what_the_books_are() {
        let books_only = DropQuery {
            can_nest: false,
            ..query(DropTargetKind::Folder, "f1", 3, 0)
        };
        assert!(matches!(
            drop_effect(books_only),
            DropEffect::NestInto { .. }
        ));
    }

    #[test]
    fn folders_and_crumbs_never_brew_a_shelf_however_long_the_rest() {
        assert!(matches!(
            drop_effect(rested(DropTargetKind::Folder, "f1", 2, 1)),
            DropEffect::NestInto { .. }
        ));
        assert!(matches!(
            drop_effect(rested(DropTargetKind::Shelf, "s1", 2, 0)),
            DropEffect::FileToShelf { .. }
        ));
        assert!(matches!(
            drop_effect(rested(DropTargetKind::Level, "s1", 2, 0)),
            DropEffect::FileToShelf { .. }
        ));
    }

    #[test]
    fn a_crumb_and_the_empty_level_are_the_same_answer() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Shelf, "s1", 1, 1)),
            DropEffect::FileToShelf {
                shelf_id: "s1".to_string()
            }
        );
        assert_eq!(
            drop_effect(query(DropTargetKind::Level, "s1", 1, 1)),
            DropEffect::FileToShelf {
                shelf_id: "s1".to_string()
            }
        );
        assert_eq!(
            drop_effect(query(DropTargetKind::Level, "", 1, 0)),
            DropEffect::FileToShelf {
                shelf_id: String::new()
            }
        );
    }

    #[test]
    fn the_ellipsis_opens_and_never_accepts() {
        assert_eq!(
            drop_effect(query(DropTargetKind::Ellipsis, "", 2, 1)),
            DropEffect::Refused
        );
        assert_eq!(
            drop_effect(rested(DropTargetKind::Ellipsis, "", 2, 1)),
            DropEffect::Refused
        );
    }

    #[test]
    fn a_band_is_only_an_after_at_the_bottom() {
        assert!(!Band::Top.after());
        assert!(!Band::Middle.after());
        assert!(Band::Bottom.after());
    }
}
