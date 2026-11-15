//! Apple Intelligence through the `fm-bridge` helper, on the Mac itself.

use fm_bridge::{Bridge, Error, Request, Schema, StreamEvent, Unavailable};
use futures_util::StreamExt;
use mareader_study::{Failure, Why};
use serde_json::Value;

use crate::backend::{Partial, forward_backend};
use crate::board::AppleState;
use crate::prompt::Prompt;

/// The on-device model; each request runs the helper once.
#[derive(Clone, Debug)]
pub struct Apple {
    bridge: Bridge,
}

impl Apple {
    pub fn new(bridge: Bridge) -> Self {
        Self { bridge }
    }

    /// The helper in the app bundle; in debug builds also `FM_BRIDGE_BIN`.
    pub fn locate() -> Result<Self, Failure> {
        let found = Bridge::bundled().or_else(|e| match cfg!(debug_assertions) {
            true => Bridge::discover(),
            false => Err(e),
        });
        found.map(Self::new).map_err(|e| failure(&e))
    }

    /// Whether the model can answer now, and if not, why.
    pub async fn probe(&self) -> AppleState {
        match self.bridge.check_availability().await {
            Ok(a) => AppleState::Ready(a.languages),
            Err(Error::ModelUnavailable { reason, message }) => match reason {
                Unavailable::ModelNotReady => AppleState::NotReady(message),
                Unavailable::NotEnabled => AppleState::Off(message),
                _ => AppleState::Missing(message),
            },
            Err(e) => AppleState::Missing(e.to_string()),
        }
    }

    async fn answer(&self, prompt: &Prompt, partial: Partial<'_>) -> Result<Value, Failure> {
        let schema = Schema::from_json(prompt.name, &prompt.schema).map_err(|e| failure(&e))?;
        let request = Request::new()
            .system(prompt.system.as_str())
            .user(prompt.user.as_str())
            .schema(schema)
            .stream_structured(true);
        let mut events = Box::pin(self.bridge.stream(request));
        let mut done = None;
        while let Some(event) = events.next().await {
            match event.map_err(|e| failure(&e))? {
                StreamEvent::Snapshot(v) => partial(v),
                StreamEvent::Structured(v) => done = Some(v),
                _ => {}
            }
        }
        done.ok_or_else(|| Failure::detailed(Why::BadAnswer, "the model sent no answer"))
    }
}

forward_backend!(Apple);

/// What the card says for `e`.
pub fn failure(e: &Error) -> Failure {
    let said = |why, m: &str| Failure::detailed(why, m);
    match e {
        Error::BinaryNotFound(_) | Error::Spawn { .. } => {
            said(Why::NotHere, "this build has no Apple Intelligence helper")
        }
        Error::ModelUnavailable { reason, message } => match reason {
            Unavailable::ModelNotReady => said(Why::NotReady, message),
            _ => said(Why::NotHere, message),
        },
        Error::GuardrailViolation(m) | Error::Refused(m) => said(Why::Refused, m),
        Error::UnsupportedLanguage(m) => said(Why::Language, m),
        Error::Busy(m) => said(Why::Busy, m),
        Error::Timeout(_) => Why::TimedOut.into(),
        Error::BadRequest(m) | Error::InvalidSchema(m) => said(Why::Setup, m),
        other => said(Why::BadAnswer, &other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn helper_errors_become_the_card_s_reasons() {
        let missing = failure(&Error::BinaryNotFound(PathBuf::from("FMBridge")));
        assert_eq!(missing.why, Why::NotHere);
        let off = Error::ModelUnavailable {
            reason: Unavailable::NotEnabled,
            message: "turn it on".into(),
        };
        assert_eq!(failure(&off).detail.as_deref(), Some("turn it on"));
        let warming = Error::ModelUnavailable {
            reason: Unavailable::ModelNotReady,
            message: "downloading".into(),
        };
        assert_eq!(failure(&warming).why, Why::NotReady);
        let lang = Error::UnsupportedLanguage("my".into());
        assert_eq!(failure(&lang).why, Why::Language);
    }
}
