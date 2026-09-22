//! The handoff between a reader and whatever hosts it.
//!
//! A reader is opened with one payload and then talks to its host over window
//! events: five frames it emits, two it accepts. This module is the payload
//! half — plain data in the shape serde puts on a wire — and says nothing
//! about how a payload travels. The names live in `ui_kit::events`, the one
//! table a Rust listener may take a window-event name from, and the pairing
//! of a name with a payload is `reader_app::wire`'s.
//!
//! Nothing dispatches these yet, and that is deliberate: a reader is still a
//! route inside the shell's own window, so the shell calls it directly and
//! has no need of a message. What is written down here is the part of a
//! boundary that cannot be recovered later. A payload shape is a type, and a
//! type the compiler checks the day something uses it; a NAME is a string two
//! programs must independently agree on, no compiler spans the gap, and the
//! only thing that notices a drift is `tools/check-events.ts`. So the names
//! and the shapes are fixed now, while the split is still one binary and
//! getting them wrong costs nothing.
//!
//! `pane_id` is the host's own handle for the instance — the id it mounted
//! the reader under — echoed back on every frame, so one host can carry
//! several readers and still know which one is talking. The reader never
//! invents it and never interprets it.

use serde::{Deserialize, Serialize};

use crate::appearance::Appearance;
use crate::format::Format;

/// What a reader is opened with. Not a window event: it is the instance's
/// construction payload, handed over once, before anything is mounted to
/// listen.
///
/// Every field is something the shell already knows and the reader cannot
/// work out for itself. What is NOT here is anything the reader can read from
/// the file — page count, outline, dimensions — because a reader that had to
/// be told those would need a second message the moment it disagreed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenParams {
    /// The address to read.
    pub path: String,
    /// Which pipeline paints it. Told rather than guessed: the shell has
    /// already resolved the extension (or the library row's), and a reader
    /// that guessed again could guess differently.
    pub format: Format,
    /// The library row this open belongs to, when it belongs to one. `None`
    /// for an address opened outside the library, which has nowhere to write
    /// a resume point and no marks to load — the same distinction
    /// `gloss_key`'s empty answer makes.
    pub book_id: Option<String>,
    /// The page to land on. `1` when there is nothing to resume.
    pub resume_page: u32,
    /// The position inside a continuous stream, which has no page to resume
    /// by. `None` for a paged read, where `resume_page` is the whole answer.
    pub resume_fraction: Option<f64>,
    /// The host's handle for this instance, echoed on every frame it emits.
    pub pane_id: String,
}

/// reader → host: mounted, painted, and ready to be told things. The host
/// waits for this before forwarding anything — an appearance broadcast sent
/// into a window that has not installed its listeners is dropped, not
/// queued.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReaderReady {
    pub pane_id: String,
}

/// reader → host: where the reading is. Emitted throttled while a session
/// runs, and once more, final, as the frame that closes it — the same shape
/// for both, because it is the same fact at two moments, and a host that
/// persisted the throttled one persists the last one identically.
///
/// `fraction` carries the continuous stream's position, which has no page
/// boundaries to resume by; `page` carries everything else. A paged read
/// leaves `fraction` at `None` rather than repeating the page as a ratio, so
/// the host can tell "no stream position" from "the top of a stream".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub pane_id: String,
    pub page: u32,
    pub fraction: Option<f64>,
}

/// reader → host: the paper colour the pane settled on, as `#rrggbb`.
///
/// The host needs it because the chrome around a document matches the
/// document: the blend backdrop, the shelf's card faces and the bar a page
/// sits under are all painted from the colour `pdf-paper` computed for this
/// reader's settings, which the host cannot compute itself without holding
/// the document's palette cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanePaper {
    pub pane_id: String,
    pub hex: String,
}

/// reader → host: this pane took focus, and it is reading this format. The
/// format rides along because what a host does with focus depends on it —
/// the shortcuts, the menus and the settings rows a reader answers to are not
/// the same for a PDF as for a stream of text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneFocus {
    pub pane_id: String,
    pub format: Format,
}

/// host → reader: the look, forwarded whole.
///
/// The whole [`Appearance`] rather than the field that moved, for the reason
/// a settings blob is a blob: every pane must end up identical, and a
/// per-field delta only converges if no frame is ever dropped. A reader
/// applies this exactly as the in-process applier does — the same tokens on
/// `<html>`, the same two pipelines over the same model — so a document
/// cannot tell whether its appearance is owned or forwarded.
///
/// Not addressed to a pane: the host broadcasts it, and every instance
/// applies it, which is what keeps N readers one look.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AppearanceBroadcast {
    pub appearance: Appearance,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::{BaseMode, NoiseMode, TextureMode};

    /// A position frame survives the wire, including the two ways "no
    /// position" is spelled: `fraction` present for a stream, absent for a
    /// paged read.
    #[test]
    fn a_position_frame_survives_the_wire() {
        let paged = Progress { pane_id: "p1".into(), page: 12, fraction: None };
        let back: Progress =
            serde_json::from_str(&serde_json::to_string(&paged).unwrap()).unwrap();
        assert_eq!(back, paged);

        let streamed = Progress { pane_id: "p1".into(), page: 1, fraction: Some(0.4375) };
        let back: Progress =
            serde_json::from_str(&serde_json::to_string(&streamed).unwrap()).unwrap();
        assert_eq!(back, streamed);
        assert_eq!(back.fraction, Some(0.4375));
    }

    /// An open carries the format and the row it belongs to, and an open
    /// outside the library carries no row — the distinction the resume point
    /// and the marks both depend on.
    #[test]
    fn an_open_names_what_it_reads_and_who_owns_it() {
        let owned = OpenParams {
            path: "/books/plate.pdf".into(),
            format: Format::Pdf,
            book_id: Some("row-7".into()),
            resume_page: 40,
            resume_fraction: None,
            pane_id: "p1".into(),
        };
        let back: OpenParams = serde_json::from_str(&serde_json::to_string(&owned).unwrap()).unwrap();
        assert_eq!(back, owned);
        assert_eq!(back.format, Format::Pdf);

        let loose = OpenParams { book_id: None, ..owned.clone() };
        let back: OpenParams = serde_json::from_str(&serde_json::to_string(&loose).unwrap()).unwrap();
        assert_eq!(back.book_id, None);
    }

    /// The look arrives WHOLE: every field of the model round-trips, so a
    /// field added to `Appearance` without a serde shape shows up here
    /// rather than as one pane silently keeping the previous grain.
    #[test]
    fn the_look_arrives_whole() {
        let appearance = Appearance {
            base: BaseMode::Dark,
            tint_hue: 38,
            tint_strength: 24,
            texture: TextureMode::Paper,
            texture_opacity: 60,
            texture_scale: 125,
            noise: NoiseMode::Static,
            noise_intensity: 9,
        };
        let frame = AppearanceBroadcast { appearance };
        let json = serde_json::to_string(&frame).unwrap();
        let back: AppearanceBroadcast = serde_json::from_str(&json).unwrap();
        assert_eq!(back, frame);
        assert_eq!(back.appearance, appearance);
    }
}
