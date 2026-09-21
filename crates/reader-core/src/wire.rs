//! The Phase-2 wire contract: the messages a reader build and a shell build
//! exchange once the reader runs in its own frame, written down (and
//! round-trip tested) BEFORE either side consumes them.
//!
//! Today the shell and the reader share one wasm module, one window and one
//! `AppState`, so most of these facts are function calls and signal writes
//! (`publish_cut`, `reading_progress`, the appearance scrub). Phase 2 gives
//! the reader its own build target in an iframe, and those calls become
//! messages: the shell posts [`OpenParams`] at open, the reader answers
//! with [`ReaderReady`], and from then on every fact the shell used to read
//! straight out of the reader's signals arrives as one of the payloads
//! below, riding the `mareader:*` window events declared in
//! `crates/ui-kit/src/events.rs`.
//!
//! Declaring them now, with zero consumers, buys the review early: the IPC
//! surface is exactly these seven events and one open message, each with a
//! serde'd payload and a direction, so Phase 2's wiring is application code
//! against a settled protocol rather than a protocol invented while wiring.
//! Nothing in the app dispatches or listens for any of this yet; the
//! round-trip tests at the bottom are the consumers, and `cargo test`
//! keeps the shapes honest until the real ones land.
//!
//! The payload structs live here (pure, host-testable) rather than in
//! `ui-kit` because they name domain types — [`Format`], the page numbers
//! the reader owns — and `ui-kit` is a leaf that knows no domain. The event
//! NAME constants stay in the one event table (`ui-kit::events`, e.g.
//! [`ui_kit::events::READER_READY_EVENT`],
//! [`ui_kit::events::READER_CLOSED_EVENT`],
//! [`ui_kit::events::PROGRESS_EVENT`],
//! [`ui_kit::events::PANE_PAPER_EVENT`],
//! [`ui_kit::events::PANE_FOCUS_EVENT`],
//! [`ui_kit::events::APPEARANCE_BROADCAST_EVENT`],
//! [`ui_kit::events::DESTROY_EVENT`]) so every event name in the app keeps
//! a single home and `tools/check-events.ts` keeps one table to police.

use serde::{Deserialize, Serialize};

use crate::format::Format;

/// The shell's open order to a reader pane: everything `document_open`
/// needs, addressed to one pane. Phase 1 delivers these facts as arguments
/// (`src/services/document/open`); Phase 2 serializes this struct into the
/// reader frame's open event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenParams {
    /// The document, exactly as the library addresses it — a path the
    /// reader build resolves itself (Tauri in the shell, a fetch or an
    /// OPFS handle in a reader-only build).
    pub path: String,
    /// Which format engine to bring up. Declared, not sniffed: the library
    /// has already fingerprinted the file.
    pub format: Format,
    /// The library row this open belongs to, when there is one — the gloss
    /// list's key and the progress row's address. `None` for an
    /// address-only open (a file dropped on the window).
    pub book_id: Option<String>,
    /// The page to land on. `0` means "no resume", not "page zero": the
    /// reader treats it as an open at the top.
    pub resume_page: u32,
    /// The stream's resume point, when the document was closed mid-stream
    /// (paged resumes keep the page, streams keep the fraction).
    pub resume_fraction: Option<f64>,
    /// Which pane the reader is speaking for. One pane today (the string
    /// the shell mints); many in Phase 2.
    pub pane_id: String,
}

/// Reader → shell, in reply to an open: this pane's reader build mounted,
/// heard the params, and is taking the document. The shell arms its
/// progress routing only after this arrives, so an open that dies mid-mount
/// cannot strand the shell listening at a pane that never came up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReaderReady {
    pub pane_id: String,
}

/// Reader → shell, on close: where the reader ended, so the shell can
/// persist progress for a document it can no longer read signals from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReaderClosed {
    pub pane_id: String,
    pub final_page: u32,
    pub final_fraction: Option<f64>,
}

/// Reader → shell, throttled by the reader's own progress plumbing: the
/// reading position as it moves. The throttle is the reader's duty — the
/// shell persists a stream of these without re-thinking it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub pane_id: String,
    pub page: u32,
    pub fraction: Option<f64>,
}

/// Reader → shell, once per appearance change: the resolved paper colour
/// this reader paints, as a hex string, so the shell can tint the frame
/// around the pane (the pane's own backdrop is the reader's business; the
/// pixels BETWEEN panes are the shell's).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanePaper {
    pub pane_id: String,
    /// The computed paper colour, `#rrggbb` — the same string the blend
    /// session publishes today.
    pub hex: String,
}

/// Reader → shell, when the pane gains or loses input focus, carrying the
/// format: global shortcuts and the title bar route by "which pane is
/// frontmost" rather than a shared mutable "current document" cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneFocus {
    pub pane_id: String,
    pub format: Format,
}

/// Shell → reader: the appearance model changed at the shell (theme
/// presets, texture, blend). The payload is the appearance JSON exactly as
/// persisted — the reader parses it with the same code its own settings
/// store uses, so the two builds cannot drift on what "appearance" means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearanceBroadcast {
    /// The persisted appearance document, as JSON text.
    pub appearance_json: String,
}

/// Shell → reader: tear this pane down. No payload — the address is the
/// event's target in Phase 2's multi-pane routing, and today's single pane
/// needs no name on a command aimed at the only reader there is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Destroy;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every payload round-trips through the JSON it will ride as. These
    /// are the contract's consumers until Phase 2 wires the real ones: a
    /// shape that stops surviving its own serialization is a shape the
    /// wire would silently corrupt.
    #[test]
    fn open_params_round_trips() {
        let p = OpenParams {
            path: "/books/odyssey.pdf".into(),
            format: Format::Pdf,
            book_id: Some("row-1".into()),
            resume_page: 12,
            resume_fraction: None,
            pane_id: "pane-0".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<OpenParams>(&json).unwrap(), p);
    }

    #[test]
    fn open_params_tolerates_an_address_only_open() {
        let p = OpenParams {
            path: "/tmp/drop.md".into(),
            format: Format::Markdown,
            book_id: None,
            resume_page: 0,
            resume_fraction: Some(0.42),
            pane_id: "pane-0".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<OpenParams>(&json).unwrap(), p);
    }

    #[test]
    fn progress_round_trips_with_and_without_a_fraction() {
        for p in [
            Progress { pane_id: "pane-0".into(), page: 3, fraction: None },
            Progress { pane_id: "pane-0".into(), page: 3, fraction: Some(0.75) },
        ] {
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(serde_json::from_str::<Progress>(&json).unwrap(), p);
        }
    }

    #[test]
    fn the_seven_events_round_trip() {
        let pane = || "pane-0".to_string();
        let cases: Vec<String> = vec![
            serde_json::to_string(&ReaderReady { pane_id: pane() }).unwrap(),
            serde_json::to_string(&ReaderClosed {
                pane_id: pane(),
                final_page: 9,
                final_fraction: None,
            })
            .unwrap(),
            serde_json::to_string(&Progress { pane_id: pane(), page: 1, fraction: None }).unwrap(),
            serde_json::to_string(&PanePaper { pane_id: pane(), hex: "#faf6ef".into() }).unwrap(),
            serde_json::to_string(&PaneFocus { pane_id: pane(), format: Format::Text }).unwrap(),
            serde_json::to_string(&AppearanceBroadcast { appearance_json: "{}".into() }).unwrap(),
            serde_json::to_string(&Destroy).unwrap(),
        ];
        for json in cases {
            serde_json::from_str::<serde_json::Value>(&json).unwrap();
        }
    }
}
