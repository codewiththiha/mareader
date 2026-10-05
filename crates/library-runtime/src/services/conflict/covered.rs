//! The two-answer question, and the two shapes that ask it: a loose import of
//! a file an in-place tree already holds a living book for, and one whose
//! content the library already holds somewhere.

use library_core::conflict::Placement;

use super::{AskKind, ConflictAsk, answer_batch, apply_placement};

/// `apply_all` is the switch beside the buttons, with the compact
/// folder-merge sheet's contract: checked, the answer is given to every
/// question of this shape in the queue. A question that is not a two-answer
/// one stops the drain.
pub fn answer_covered(state: crate::context::LibraryContext, answer: Placement, apply_all: bool) {
    answer_batch(state, answer, apply_all, AskKind::is_two_answer, apply_one);
}

fn apply_one(state: crate::context::LibraryContext, ask: &ConflictAsk, answer: Placement) {
    apply_placement(state, &ask.placement(state), answer);
}
