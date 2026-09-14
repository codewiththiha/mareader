//! The two-answer question, and the two shapes that ask it: a loose import of a file an
//! in-place folder's tree already holds a living book for, and a loose import of a file
//! whose CONTENT the library already holds somewhere.

use library_core::conflict::Placement;

use super::{answer_batch, apply_placement, AskKind, ConflictAsk};
use crate::state::AppState;

/// `apply_all` is the switch beside the buttons, with the compact folder-merge sheet's
/// contract: checked, the answer is given to every question of this shape in the queue.
/// A question that is NOT a two-answer one stops the drain.
pub fn answer_covered(state: AppState, answer: Placement, apply_all: bool) {
    answer_batch(
        state,
        answer,
        apply_all,
        AskKind::is_two_answer,
        apply_one,
    );
}

fn apply_one(state: AppState, ask: &ConflictAsk, answer: Placement) {
    apply_placement(state, &ask.placement(state), answer);
}
