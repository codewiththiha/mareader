//! The reader's side of its contract with whatever hosts it.
//!
//! A reader and its host are separate programs with no shared memory: the
//! host cannot reach into a reader's state and ask, and a reader cannot hold
//! a signal from the host. Everything between them is one of these frames,
//! dispatched as a `window` CustomEvent under a name from
//! [`ui_kit::events`] with a payload from [`reader_core::wire`].
//!
//! Two tables, and they are the whole interface. The names are constants
//! because a name is the one thing a compiler cannot check across two
//! programs: `reader-app` dispatches the string, the host listens for it, and
//! if they disagree the frame goes into a window nobody is on.
//!
//! Nothing dispatches these yet — a reader is still a route in the shell's own
//! window, so the shell still calls it directly — but the pairing is written
//! down with tests rather than left to the day it is needed, because the day
//! it is needed is the day two programs start agreeing by string and there is
//! nothing else that would have caught them disagreeing.
//!
//! The shape of the interface is deliberately asymmetric: five frames out,
//! two in. A reader owns everything about reading and reports the parts of it
//! a host needs (where the reading is, what the pane looks like, that focus
//! moved), and a host owns the look and the lifecycle. There is no frame by
//! which a host tells a reader to navigate, because navigation is reading and
//! the reader is the one that knows whether a spot still exists.

/// The frames a reader sends, each paired with its payload.
pub const EMITS: &[(&str, &str)] = &[
    (
        ui_kit::events::READER_READY_EVENT,
        "reader_core::wire::ReaderReady",
    ),
    (
        ui_kit::events::READER_CLOSED_EVENT,
        "reader_core::wire::Progress",
    ),
    (ui_kit::events::PROGRESS_EVENT, "reader_core::wire::Progress"),
    (
        ui_kit::events::PANE_PAPER_EVENT,
        "reader_core::wire::PanePaper",
    ),
    (
        ui_kit::events::PANE_FOCUS_EVENT,
        "reader_core::wire::PaneFocus",
    ),
];

/// The frames a reader accepts. `Progress` appears in both tables because it
/// is the payload type, not the frame: the same fact crosses the boundary on
/// its way out of the reader, and nothing stops a host from echoing it back
/// into a pane it just re-opened.
pub const ACCEPTS: &[(&str, &str)] = &[
    (
        ui_kit::events::APPEARANCE_BROADCAST_EVENT,
        "reader_core::wire::AppearanceBroadcast",
    ),
    (ui_kit::events::DESTROY_EVENT, "()"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Names are what a host listens for, so a duplicate here would mean one
    /// of the two frames silently never reaches its listener: two emitters on
    /// one name, and whichever the host wired up first wins.
    #[test]
    fn emits_and_accepts_are_distinct() {
        let mut names: Vec<&str> = EMITS.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), EMITS.len());

        let mut names: Vec<&str> = ACCEPTS.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ACCEPTS.len());
    }

    /// The reader never listens for a frame it sends. A reader that did would
    /// hear its own dispatch — `window` events bubble to the dispatching
    /// window's own listeners — and would then act on a report it made, which
    /// is a feedback loop rather than a conversation.
    #[test]
    fn emits_and_accepts_do_not_overlap() {
        for (name, _) in EMITS {
            assert!(
                !ACCEPTS.iter().any(|(n, _)| n == name),
                "{name} is both emitted and accepted"
            );
        }
    }

    /// Every payload named by a frame is one the reader can actually build.
    /// The names in these tables are strings, so this is the only place a
    /// misspelling shows up: the compiler cannot see it, and a wrong name
    /// would mean the reader dispatches a payload the host cannot deserialize.
    #[test]
    fn named_payloads_are_the_ones_reader_core_declares() {
        let known = [
            "reader_core::wire::OpenParams",
            "reader_core::wire::ReaderReady",
            "reader_core::wire::Progress",
            "reader_core::wire::PanePaper",
            "reader_core::wire::PaneFocus",
            "reader_core::wire::AppearanceBroadcast",
            "()",
        ];
        for frame in EMITS.iter().chain(ACCEPTS) {
            assert!(
                known.contains(&frame.1),
                "unknown payload for {}: {}",
                frame.0,
                frame.1
            );
        }
    }

    /// The round trip that matters, against the real type rather than a
    /// description of it: a frame this crate dispatches is the frame a host
    /// reads back. `reader-closed` carries the same shape as a throttled
    /// `progress` frame, so a host that stored the last one and replays it as
    /// the other has to get the identical fact.
    #[test]
    fn progress_survives_the_round_trip() {
        let sent = reader_core::wire::Progress {
            pane_id: "p1".into(),
            page: 12,
            fraction: Some(0.4375),
        };
        let json = serde_json::to_string(&sent).expect("serialize");
        let back: reader_core::wire::Progress = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, sent);
    }
}
