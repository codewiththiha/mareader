//! The wire protocol between the Tauri backend and the frontend: the chunk
//! stream ([`AiChunk`]) and the typed error it can carry ([`AiError`]).
//!
//! The frontend mirrors both in `src/components/ai/types.rs` and
//! `src/services/ai.rs` — keep the serde shapes in sync (the test below pins
//! this crate's half of the contract).

use futures::Stream;
use std::pin::Pin;

use super::schema::WordInfo;

/// Machine-readable cause of an [`AiError`]. Branch on this, never on the
/// human-facing `message`, whose wording may change between OS releases.
///
/// Serializes flat — unit variants as `"snake_case"` strings — so the wire
/// shape stays `{"kind":"model_not_ready"}` rather than nesting tags.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    not(all(feature = "ai", target_os = "macos", target_arch = "aarch64")),
    allow(dead_code)
)]
pub enum AiErrorKind {
    /// Apple Intelligence is switched off in System Settings; the user can fix it.
    NotEnabled,
    /// Model assets are still downloading/preparing; transient.
    ModelNotReady,
    /// The hardware can never run Apple Intelligence.
    DeviceNotEligible,
    /// macOS too old / Foundation Models absent.
    OsTooOld,
    /// Safety guardrails blocked the request or the response.
    BlockedByGuardrail,
    /// Prompt + response exceeded the context window.
    ContextTooLong,
    /// The request exceeded the bridge timeout (queue wait or generation).
    Timeout,
    /// The model is already responding to another request.
    Busy,
    /// The response did not match the WordInfo schema.
    BadResponse,
    /// Anything else; carries a short user-facing summary.
    Other(String),
}

/// A typed error from the AI pipeline, serialized across the wire so the
/// frontend can show the right message and a retry affordance exactly when
/// retrying might help.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AiError {
    /// Machine-readable cause — the frontend's branch point.
    pub kind: AiErrorKind,
    /// Human-readable detail. For [`AiErrorKind::Other`] this is written to
    /// be shown directly (bounded in length); for the named kinds it is the
    /// full provider/bridge text, kept for logs and devtools.
    pub message: String,
    /// Mirrors `fm_bridge::Error::is_retryable` (plus schema-shape faults).
    pub retryable: bool,
}

/// The chunks of data we will stream to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AiChunk {
    /// A partial or final snapshot of the WordInfo
    Snapshot(WordInfo),
    /// The stream is complete
    Done,
    /// The run failed; carries a typed, retryable-aware error.
    Error(AiError),
}

/// The trait that all AI providers must implement.
/// We use a pinned boxed stream to allow async streaming without lifetime hell.
pub trait AiProvider: Send + Sync {
    fn explain_word(
        &self,
        word: String,
        context: String,
    ) -> Pin<Box<dyn Stream<Item = AiChunk> + Send>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend's `AiChunkEvent` mirror must parse exactly these shapes.
    #[test]
    fn error_chunk_serializes_with_a_flat_kind() {
        let chunk = AiChunk::Error(AiError {
            kind: AiErrorKind::ModelNotReady,
            message: "the on-device model is still downloading".into(),
            retryable: true,
        });
        let json = serde_json::to_value(&chunk).unwrap();
        assert_eq!(json["type"], "Error");
        assert_eq!(json["data"]["kind"], "model_not_ready");
        assert_eq!(json["data"]["retryable"], true);

        // The escape-hatch variant carries its summary inline.
        let other = AiChunk::Error(AiError {
            kind: AiErrorKind::Other("helper crashed".into()),
            message: "helper crashed".into(),
            retryable: false,
        });
        let json = serde_json::to_value(&other).unwrap();
        assert_eq!(json["data"]["kind"]["other"], "helper crashed");
    }
}
