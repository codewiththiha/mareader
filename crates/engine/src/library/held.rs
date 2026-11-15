//! Books a filing run holds, and the actions it will not let them take.

use std::collections::BTreeSet;

use mareader_library::BookId;

use super::{Out, Pick, ShelfAction, State};
use crate::toast::Toast;

const HELD: &str = "Books the AI is organizing stay put until it's done.";

impl State {
    /// Books a filing run holds: they open, but they don't move.
    pub fn held(&self) -> &BTreeSet<BookId> {
        &self.held
    }

    /// Whether a filing run holds any book.
    pub fn busy(&self) -> bool {
        !self.held.is_empty()
    }

    pub fn unheld(&self, action: ShelfAction, out: &mut Vec<Out>) -> Option<ShelfAction> {
        let held = &self.held;
        if held.is_empty() {
            return Some(action);
        }
        let free = |p: &Pick| !matches!(p, Pick::Book(b) if held.contains(b));
        let (action, dropped) = match action {
            ShelfAction::Carry(mut mv) => {
                let n = mv.books.len();
                mv.books.retain(|b| !held.contains(b));
                let dropped = mv.books.len() < n;
                (Some(ShelfAction::Carry(mv)), dropped)
            }
            ShelfAction::Stack {
                onto,
                mut carry,
                level,
                keep,
            } => {
                let n = carry.items.len();
                carry.items.retain(free);
                let dropped = held.contains(&onto) || carry.items.len() < n;
                let stack = ShelfAction::Stack {
                    onto,
                    carry,
                    level,
                    keep,
                };
                ((!dropped).then_some(stack), dropped)
            }
            other => (Some(other), false),
        };
        if dropped {
            out.push(Out::Toast(Toast::info(HELD)));
        }
        action
    }

    /// Picks without the books a run holds; a toast says why.
    pub fn unheld_picks(&self, mut picks: Vec<Pick>, out: &mut Vec<Out>) -> Vec<Pick> {
        let held = &self.held;
        let n = picks.len();
        picks.retain(|p| !matches!(p, Pick::Book(b) if held.contains(b)));
        if picks.len() < n {
            out.push(Out::Toast(Toast::info(HELD)));
        }
        picks
    }
}
