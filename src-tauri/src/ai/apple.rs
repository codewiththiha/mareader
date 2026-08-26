//! The real provider: Apple Intelligence via the `fm-bridge` helper.
//!
//! Every `fm_bridge::Error` is mapped onto a typed [`AiError`] before it
//! crosses the wire, so the frontend can branch on the *cause* (and show a
//! retry affordance exactly when `fm_bridge` says retrying might help)
//! instead of string-matching on prose that may change between OS releases.

use async_stream::stream;
use fm_bridge::{Bridge, Error as BridgeError, Request, StreamEvent, Unavailable};
use futures::{Stream, StreamExt};
use std::pin::Pin;
use std::time::Duration;

use super::{
    prompts::{WORD_INFO_SYSTEM_PROMPT, WORD_INFO_USER_PROMPT},
    schema::{word_info_schema, WordInfo},
    traits::{AiChunk, AiError, AiErrorKind, AiProvider},
};

pub struct AppleAiProvider {
    bridge: Bridge,
}

impl AppleAiProvider {
    pub fn new() -> Result<Self, String> {
        // Reads FM_BRIDGE_BIN from the .env file.
        //
        // Concurrency stays small on purpose: the on-device model is one
        // shared resource and each slot is its own helper process. The
        // timeout covers queue wait AND generation, so a stuck or saturated
        // model surfaces as a retryable Timeout instead of hanging the UI.
        let bridge = Bridge::from_env()
            .map_err(|e| e.to_string())?
            .max_concurrency(2)
            .timeout(Duration::from_secs(90));

        Ok(Self { bridge })
    }
}

/// Maps a typed bridge error onto the wire error. Retryability comes
/// straight from `fm_bridge` (ModelNotReady / Timeout / "already responding"
/// only), with schema-shape failures treated as worth one more try.
fn map_err(e: BridgeError) -> AiError {
    let retryable = e.is_retryable();
    let kind = match &e {
        BridgeError::ModelUnavailable { reason, .. } => match reason {
            Unavailable::NotEnabled => AiErrorKind::NotEnabled,
            Unavailable::ModelNotReady => AiErrorKind::ModelNotReady,
            Unavailable::DeviceNotEligible => AiErrorKind::DeviceNotEligible,
            Unavailable::OsTooOld => AiErrorKind::OsTooOld,
            // Unknown today, or introduced by a newer SDK.
            _ => AiErrorKind::Other("the on-device model is unavailable".into()),
        },
        BridgeError::GuardrailViolation(_) => AiErrorKind::BlockedByGuardrail,
        BridgeError::ContextExceeded(_) => AiErrorKind::ContextTooLong,
        BridgeError::Timeout(_) => AiErrorKind::Timeout,
        BridgeError::Generation(m) if m.contains("already responding") => AiErrorKind::Busy,
        // Request/schema faults on OUR side, not the model's.
        BridgeError::BadRequest(_) | BridgeError::InvalidSchema(_) => AiErrorKind::BadResponse,
        other => {
            // Keep the full text in the logs; only a bounded summary
            // travels to the UI (and `Other` shows `message` directly).
            eprintln!("[ai] unmapped bridge error: {other}");
            AiErrorKind::Other(short(other))
        }
    };
    let message = match &kind {
        AiErrorKind::Other(summary) => summary.clone(),
        _ => e.to_string(),
    };
    AiError {
        kind,
        message,
        retryable,
    }
}

/// Bounded user-facing summary for the escape-hatch variant.
fn short(e: &BridgeError) -> String {
    const MAX: usize = 80;
    let s = e.to_string();
    if s.chars().count() <= MAX {
        s
    } else {
        s.chars().take(MAX).collect::<String>() + "…"
    }
}

impl AiProvider for AppleAiProvider {
    fn explain_word(
        &self,
        word: String,
        context: String,
    ) -> Pin<Box<dyn Stream<Item = AiChunk> + Send>> {
        let bridge = self.bridge.clone();
        let schema = word_info_schema();

        let prompt = WORD_INFO_USER_PROMPT
            .replace("{word}", &word)
            .replace("{context}", &context);

        let request = Request::new()
            .system(WORD_INFO_SYSTEM_PROMPT)
            .user(&prompt)
            .schema(schema)
            .stream_structured(true); // Enable streaming of JSON snapshots

        Box::pin(stream! {
            let mut stream = Box::pin(bridge.stream(request));
            // Early snapshots are partial objects and routinely fail to
            // parse — that is normal. The FINAL structured payload must
            // parse, though: if the stream ends without one usable value
            // the UI would otherwise show an empty card with no recourse.
            let mut saw_usable = false;

            while let Some(event) = stream.next().await {
                match event {
                    Ok(StreamEvent::Snapshot(val)) | Ok(StreamEvent::Structured(val)) => {
                        if let Ok(info) = serde_json::from_value::<WordInfo>(val) {
                            saw_usable = true;
                            yield AiChunk::Snapshot(info);
                        }
                    }
                    Ok(StreamEvent::Done(_)) => {
                        if saw_usable {
                            yield AiChunk::Done;
                        } else {
                            yield AiChunk::Error(AiError {
                                kind: AiErrorKind::BadResponse,
                                message: "the model's response did not match the WordInfo schema".into(),
                                retryable: true,
                            });
                        }
                        break;
                    }
                    Err(e) => {
                        yield AiChunk::Error(map_err(e));
                        break;
                    }
                    _ => {} // Ignore other events
                }
            }
        })
    }
}
