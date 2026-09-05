//! The real provider: Apple Intelligence via the `fm-bridge` helper.
//!
//! Every `fm_bridge::Error` is mapped onto a typed [`AiError`] before it
//! crosses the wire, so the frontend can branch on the *cause* (and show a
//! retry affordance exactly when `fm_bridge` says retrying might help)
//! instead of string-matching on prose that may change between OS releases.

use async_stream::stream;
use fm_bridge::Bridge;
use fm_bridge::Error as BridgeError;
use fm_bridge::{Request, StreamEvent, Unavailable};
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

/// Fold whatever fields a (partial) snapshot already contains into the
/// running answer. Later chunks only overwrite a field once it has real
/// content, so a chunk that echoes an empty array never wipes the synonyms
/// that arrived earlier.
fn merge_partial(acc: &mut WordInfo, val: &serde_json::Value) {
    if let Some(s) = val.get("pos").and_then(|v| v.as_str()) {
        if !s.trim().is_empty() {
            acc.pos = s.to_string();
        }
    }
    if let Some(s) = val.get("meaning").and_then(|v| v.as_str()) {
        if !s.trim().is_empty() {
            acc.meaning = s.to_string();
        }
    }
    if let Some(a) = val.get("synonyms").and_then(|v| v.as_array()) {
        let items: Vec<String> = a.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        if !items.is_empty() {
            acc.synonyms = items;
        }
    }
    if let Some(a) = val.get("usages").and_then(|v| v.as_array()) {
        let items: Vec<String> = a.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        if !items.is_empty() {
            acc.usages = items;
        }
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
            // The UI wants a card to open the MOMENT the first real content
            // arrives, not a run later. Structured streaming hands us the
            // object as it is written, so the early snapshots are partial —
            // a field or two, nothing that parses as a whole `WordInfo`.
            // Instead of dropping those until the object happens to be
            // complete (which deferred the card until the model was nearly
            // finished), we accumulate the partial fields and surface the
            // running answer on the FIRST chunk that carries a `meaning`,
            // then re-publish it as each later chunk fills in more. The
            // frontend patches the sections in place, so the card streams.
            let mut acc = WordInfo::default();
            // The FINAL structured payload must still carry real content: if
            // the stream ends before a `meaning` ever landed, that is a
            // shape failure — the UI would otherwise show an empty card
            // with no recourse.
            let mut saw_usable = false;

            while let Some(event) = stream.next().await {
                match event {
                    Ok(StreamEvent::Snapshot(val)) | Ok(StreamEvent::Structured(val)) => {
                        merge_partial(&mut acc, &val);
                        if !acc.meaning.trim().is_empty() {
                            saw_usable = true;
                            yield AiChunk::Snapshot(acc.clone());
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
