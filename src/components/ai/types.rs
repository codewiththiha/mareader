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

/// Bounds on a single answer, applied at ingestion. A well-behaved model
/// answers in a sentence or two; these are the sizes past which a response is
/// no longer an answer but a runaway generation, and the card holds it for the
/// rest of the session (every mark's answer stays cached until its mark is
/// removed). Clipping at the door keeps that ceiling flat instead of letting
/// one pathological run set it.
const MAX_MEANING_CHARS: usize = 1_200;
const MAX_SYNONYMS: usize = 16;
const MAX_SYNONYM_CHARS: usize = 80;
const MAX_USAGES: usize = 8;
const MAX_USAGE_CHARS: usize = 400;

/// The ellipsis that marks a clipped string, so a truncated answer reads as
/// truncated rather than as a model that stopped mid-word.
const ELLIPSIS: char = '\u{2026}';

/// Clip `s` to at most `max` characters (not bytes — this is human text and
/// the boundary must be a char boundary), marking the cut.
fn clamp_text(s: String, max: usize) -> String {
    match s.char_indices().nth(max) {
        None => s,
        Some((byte, _)) => {
            let mut clipped = s;
            clipped.truncate(byte);
            clipped.push(ELLIPSIS);
            clipped
        }
    }
}

impl WordInfo {
    /// The answer bounded to the sizes above. Applied once, where snapshots
    /// enter the app, so nothing downstream — the card, the measure twin, the
    /// session cache — has to reason about how big an answer can be.
    pub fn clamped(self) -> Self {
        Self {
            pos: clamp_text(self.pos, MAX_SYNONYM_CHARS),
            meaning: clamp_text(self.meaning, MAX_MEANING_CHARS),
            synonyms: self
                .synonyms
                .into_iter()
                .take(MAX_SYNONYMS)
                .map(|s| clamp_text(s, MAX_SYNONYM_CHARS))
                .collect(),
            usages: self
                .usages
                .into_iter()
                .take(MAX_USAGES)
                .map(|u| clamp_text(u, MAX_USAGE_CHARS))
                .collect(),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_answer_passes_through_untouched() {
        let info = WordInfo {
            pos: "adjective".into(),
            meaning: "lasting for a very short time".into(),
            synonyms: vec!["fleeting".into(), "transient".into()],
            usages: vec!["ephemeral pleasures".into()],
        };
        assert_eq!(info.clone().clamped(), info);
    }

    #[test]
    fn a_runaway_answer_is_clipped_at_every_axis() {
        let clamped = WordInfo {
            pos: "noun".into(),
            meaning: "x".repeat(MAX_MEANING_CHARS + 500),
            synonyms: (0..MAX_SYNONYMS + 10).map(|_| "y".repeat(200)).collect(),
            usages: (0..MAX_USAGES + 5).map(|_| "z".repeat(1_000)).collect(),
        }
        .clamped();

        assert_eq!(clamped.meaning.chars().count(), MAX_MEANING_CHARS + 1);
        assert!(clamped.meaning.ends_with(ELLIPSIS));
        assert_eq!(clamped.synonyms.len(), MAX_SYNONYMS);
        assert_eq!(clamped.synonyms[0].chars().count(), MAX_SYNONYM_CHARS + 1);
        assert_eq!(clamped.usages.len(), MAX_USAGES);
        assert_eq!(clamped.usages[0].chars().count(), MAX_USAGE_CHARS + 1);
    }

    #[test]
    fn clipping_lands_on_a_char_boundary() {
        // Multi-byte input: truncating by bytes here would panic.
        let text: String = "é".repeat(MAX_MEANING_CHARS + 20);
        let clipped = clamp_text(text, MAX_MEANING_CHARS);
        assert_eq!(clipped.chars().count(), MAX_MEANING_CHARS + 1);
    }

    #[test]
    fn a_string_exactly_at_the_bound_is_not_marked() {
        let text = "a".repeat(MAX_MEANING_CHARS);
        assert_eq!(clamp_text(text.clone(), MAX_MEANING_CHARS), text);
    }
}
