//! Zoom commands and the one question the watchers ask about them.

use crate::state::{ZoomCommand, ZoomTransition};

/// Does this command hold its crisp commit until the container
/// settles?
pub(crate) fn holds_commit(cmd: ZoomCommand) -> bool {
    matches!(cmd, ZoomCommand::Follow)
}

/// What a container-driven watcher may do, given the open
/// transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    /// Nothing is open: post the command as its own transaction.
    Now,
    /// A follow holds the burst: retarget it.
    Follow,
    /// A tweened gesture owns the transaction: post nothing.
    StandDown,
}

/// "Is a zoom in flight" alone would make a slide jump at the end.
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
        // Idle: a slide opens its own transaction.
        assert_eq!(posting_gate(None), Gate::Now);
        // A follow in flight is retargeted every frame of the burst.
        assert_eq!(posting_gate(Some(open(true))), Gate::Follow);
        // A tweened gesture keeps its transaction to itself.
        assert_eq!(posting_gate(Some(open(false))), Gate::StandDown);
    }
}
