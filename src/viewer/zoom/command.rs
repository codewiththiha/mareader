//! Zoom commands and the one question the watchers ask about them.
//!
//! A command is an INTENT — "step in", "re-fit", "the container moved" — and
//! it is deliberately not a scale. It travels on a signal
//! ([`crate::state::reader::ZoomCommand`], which lives with the rest of the
//! reactive shape) and the coordinator is its only consumer, so a zoom can
//! never be executed by writing the scale signals from the side.
//!
//! The two decisions ABOUT a command are here, apart from the machinery that
//! acts on them, because both are pure and both are the whole insight of the
//! watcher split:
//!
//! * [`holds_commit`] — whether the command's crisp render waits for the
//!   container to settle. Only a follow's does.
//! * [`posting_gate`] — whether a container-driven watcher may post at all,
//!   given the transaction that is already open. This is the gate the two
//!   watchers differ on, and it is what keeps a sidebar slide from snapping a
//!   reader's in-flight pinch.

use crate::state::reader::{ZoomCommand, ZoomTransition};

/// Does this command hold its crisp commit until the container settles?
///
/// Only a container follow does. Everything else is a single, deliberate change
/// of scale and commits as soon as it lands — deferring it would leave the page
/// stretched for no reason.
pub(crate) fn holds_commit(cmd: ZoomCommand) -> bool {
    matches!(cmd, ZoomCommand::Follow)
}

/// What a container-driven watcher is allowed to do, given the transaction that
/// is open. The watchers ask this instead of reading the transition themselves,
/// so "who owns the scale right now" stays a pipeline question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Nothing is open: post the command as its own transaction, which lands and
    /// commits on the same frame.
    Now,
    /// A follow already holds the burst: retarget it. Its commit is still the
    /// settle deadline's, so a change that arrives mid-burst (a page turn under a
    /// fit, say) rides the same single render instead of forcing its own.
    Follow,
    /// A tweened gesture owns the transaction. Post nothing: resolving container
    /// maths into it and writing the display scale would land the scale BEFORE
    /// the tween had anything to interpolate from — the one-frame snap that made
    /// the zoom control look broken.
    StandDown,
}

/// The gate for a transaction that may be open.
///
/// Keying this off "is a zoom in flight" alone would be just as wrong as ignoring
/// it: the first frame of a slide opens the transaction, so every later frame
/// would bail out and the smooth slide would become a jump at the end.
pub(crate) fn posting_gate(open: Option<ZoomTransition>) -> Gate {
    match open {
        None => Gate::Now,
        Some(t) if t.following => Gate::Follow,
        Some(_) => Gate::StandDown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(following: bool) -> ZoomTransition {
        ZoomTransition {
            from: 1.0,
            to: 1.4,
            start_ms: 0.0,
            animate: !following,
            following,
        }
    }

    #[test]
    fn only_a_follow_holds_its_render() {
        assert!(holds_commit(ZoomCommand::Follow));
        for cmd in [
            ZoomCommand::Step(1),
            ZoomCommand::Refit,
            ZoomCommand::Constrain,
        ] {
            assert!(!holds_commit(cmd), "{cmd:?} is one deliberate change");
        }
    }

    #[test]
    fn a_slide_may_retarget_itself_but_never_a_gesture() {
        // Idle: a slide may open its own transaction, and a discrete refit
        // commits as soon as it lands.
        assert_eq!(posting_gate(None), Gate::Now);
        // A follow already in flight is retargeted on every frame of the burst,
        // and anything resolving in the meantime rides its held commit.
        assert_eq!(posting_gate(Some(open(true))), Gate::Follow);
        // A tweened gesture keeps its transaction to itself.
        assert_eq!(posting_gate(Some(open(false))), Gate::StandDown);
    }
}
