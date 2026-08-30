use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// Geometry of the word card. Orthogonal to [`AiPhase`] (the AI data status):
/// `AiPhase` says what the *data* is doing, `GlossPhase` says what the
/// *card's box* is doing. The two are decoupled so the card body can stream
/// text while the box independently expands / folds back onto the stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlossPhase {
    /// Exact-fit stroke hugging the selected word. No surface is mounted in
    /// this phase: the in-page highlighter's thinking pulse is the whole
    /// waiting UI.
    #[default]
    Processing,
    /// The full card, sprung open from the word.
    Expanded,
    /// Folded back onto the word (scroll-to-close) — a chip the reader can
    /// hover/tap to re-open in place.
    Compact,
}

/// The data status of the AI explanation: what the *content* of the card is
/// doing, independent of the card's geometry phase ([`GlossPhase`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiPhase {
    /// No AI activity. The card is closed.
    #[default]
    Idle,
    /// The AI request has been sent; waiting for the first token. Only the
    /// highlighter stroke's processing animation is on screen.
    Processing,
    /// Tokens are arriving. The card is open and text is streaming in; each
    /// snapshot PATCHES the sections in place (a fade-in runs once, on
    /// mount, never per chunk).
    Streaming,
    /// All data received. Final state.
    Done,
    /// Something went wrong.
    Error,
}

/// The structured word information returned by the AI.
/// Mirrors the backend `WordInfo` struct.
///
/// `Serialize` is required so the app-lifetime AI chunk bridge can
/// re-broadcast snapshots as a window `CustomEvent` for the popover.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordInfo {
    pub pos: String,
    pub meaning: String,
    pub synonyms: Vec<String>,
    pub usages: Vec<String>,
}

/// Mirror of the backend's `AiErrorKind` (`src-tauri/src/ai/traits.rs`) —
/// keep the serde shapes in sync. Branch on `kind`, never on `message`
/// wording: the prose may change between OS releases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiErrorKind {
    NotEnabled,
    ModelNotReady,
    DeviceNotEligible,
    OsTooOld,
    BlockedByGuardrail,
    ContextTooLong,
    Timeout,
    Busy,
    BadResponse,
    Other(String),
}

/// Mirror of the backend's `AiError`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiError {
    pub kind: AiErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl AiError {
    /// Short, human explanation mapped once per kind so the wording stays
    /// consistent everywhere it is shown. [`AiErrorKind::Other`] is the
    /// escape hatch whose `message` was written to be user-facing (and is
    /// bounded in length by whoever constructs it).
    pub fn friendly(&self) -> Cow<'_, str> {
        match &self.kind {
            AiErrorKind::NotEnabled => "Apple Intelligence is turned off. Turn it on in System Settings → Apple Intelligence.".into(),
            AiErrorKind::ModelNotReady => {
                "The on-device model is still downloading. Try again in a little while.".into()
            }
            AiErrorKind::DeviceNotEligible => "This Mac doesn't support Apple Intelligence.".into(),
            AiErrorKind::OsTooOld => "macOS 26 or newer is required for on-device explanations.".into(),
            AiErrorKind::BlockedByGuardrail => "The model declined to answer this one.".into(),
            AiErrorKind::ContextTooLong => "The selected passage is too long to analyze.".into(),
            AiErrorKind::Timeout => "The model took too long to respond.".into(),
            AiErrorKind::Busy => "The model is busy with another request right now.".into(),
            AiErrorKind::BadResponse => "The response didn't match the expected format.".into(),
            AiErrorKind::Other(_) => self.message.as_str().into(),
        }
    }

    /// Fallback for render paths that must show *something* even if the
    /// error signal was (impossibly) cleared between phase and paint.
    pub fn unknown() -> Self {
        Self {
            kind: AiErrorKind::Other("unknown".into()),
            message: "Something went wrong.".into(),
            retryable: false,
        }
    }
}
