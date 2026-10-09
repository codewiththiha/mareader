//! What one download reports, and what it hands back when it lands.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Where a download is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The destination is being prepared; no byte has moved.
    Preparing,
    /// Bytes are arriving.
    Downloading,
    /// An attempt is spent; the backoff before the next one is running.
    Retrying,
    /// The caller stopped the read; the partial stays for `resume`.
    Paused,
    /// Every byte is here; the verify hook is running.
    Verifying,
    /// The file is complete and adopted at its destination.
    Done,
    /// Every mirror is out of attempts.
    Failed,
    /// The caller asked to stop; the partial stays for a later `start`.
    Cancelled,
}

impl Phase {
    /// No further progress will be reported for this download.
    pub fn terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

/// One download's state, in the shape a frontend draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: String,
    pub phase: Phase,
    pub received: u64,
    pub total: Option<u64>,
    /// The mirror answering now; `None` before the first response.
    pub source: Option<u32>,
    /// The attempt inside that mirror, from one.
    pub attempt: u32,
    /// Bytes per second, smoothed; `None` until a window has passed.
    pub speed: Option<f64>,
    /// Whole seconds left at `speed`, when it and `total` are known.
    pub eta_secs: Option<u64>,
    /// Why this phase: the last failure, the retry's reason.
    pub message: Option<String>,
    /// The adopted file; `Done` only.
    pub path: Option<PathBuf>,
    /// `Done` without a byte moving: the file was already complete.
    pub cached: bool,
}

impl Progress {
    pub(crate) fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            phase: Phase::Preparing,
            received: 0,
            total: None,
            source: None,
            attempt: 0,
            speed: None,
            eta_secs: None,
            message: None,
            path: None,
            cached: false,
        }
    }

    /// The whole percent done, when the size is known and not zero.
    pub fn percent(&self) -> Option<u8> {
        let total = self.total?;
        (total > 0).then(|| ((self.received.min(total) * 100) / total) as u8)
    }
}

/// What a finished download hands back to whoever asked.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub id: String,
    /// The adopted file: complete, verified, at its final name.
    pub path: PathBuf,
    pub bytes: u64,
    /// The mirror that served it.
    pub source: u32,
    /// Nothing moved: the destination was already complete.
    pub cached: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_three_endings_are_terminal() {
        for phase in [Phase::Done, Phase::Failed, Phase::Cancelled] {
            assert!(phase.terminal());
        }
        for phase in [
            Phase::Preparing,
            Phase::Downloading,
            Phase::Retrying,
            Phase::Paused,
            Phase::Verifying,
        ] {
            assert!(!phase.terminal());
        }
    }

    #[test]
    fn percent_needs_a_size_and_never_passes_a_hundred() {
        let mut progress = Progress::new("x");
        assert_eq!(progress.percent(), None);
        progress.total = Some(0);
        progress.received = 10;
        assert_eq!(progress.percent(), None);
        progress.total = Some(200);
        progress.received = 50;
        assert_eq!(progress.percent(), Some(25));
        // A server that under-declares cannot push the bar past full.
        progress.received = 400;
        assert_eq!(progress.percent(), Some(100));
    }

    #[test]
    fn the_wire_shape_is_camel_case_over_snake_case_phases() {
        let mut progress = Progress::new("cefr-dataset");
        progress.phase = Phase::Downloading;
        progress.received = 1024;
        progress.total = Some(2048);
        progress.eta_secs = Some(3);
        let json = serde_json::to_string(&progress).unwrap();
        assert!(json.contains("\"phase\":\"downloading\""));
        assert!(json.contains("\"etaSecs\":3"));
        assert!(!json.contains("eta_secs"));
        assert!(!json.contains("file_name"));
        // A frontend mirrors the same shape back.
        let parsed: Progress = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, progress);
    }

    #[test]
    fn a_path_serializes_as_a_string() {
        let mut progress = Progress::new("x");
        progress.phase = Phase::Done;
        progress.path = Some(PathBuf::from("/tmp/cefr/cefr.parquet"));
        let json = serde_json::to_string(&progress).unwrap();
        assert!(json.contains("\"path\":\"/tmp/cefr/cefr.parquet\""));
    }
}
