//! The two-answer question and its two shapes: an in-place tree's book, or

use library_core::conflict::Placement;

use super::{AskKind, ConflictAsk, answer_batch, apply_placement};

/// The switch beside the buttons: checked, it answers every such question.
pub fn answer_covered(state: crate::context::LibraryContext, answer: Placement, apply_all: bool) {
    answer_batch(state, answer, apply_all, AskKind::is_two_answer, apply_one);
}

fn apply_one(state: crate::context::LibraryContext, ask: &ConflictAsk, answer: Placement) {
    apply_placement(state, &ask.placement(state), answer);
}
