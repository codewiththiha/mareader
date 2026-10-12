//! The vocabulary dataset's lifecycle, as the backend reports it.

use serde::{Deserialize, Serialize};

/// The dataset's lifecycle, as the backend reports it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DatasetMirror {
    /// `absent` | `downloading` | `paused` | `converting` | `ready` |
    /// `failed`.
    pub phase: String,
    pub received: u64,
    pub total: Option<u64>,
    /// Bytes per second, smoothed; the backend stops sending it when idle.
    pub speed: Option<f64>,
    /// Whole seconds left, when the size and the speed are both known.
    pub eta_secs: Option<u64>,
    pub words: Option<u64>,
    pub message: Option<String>,
    /// The click-time tagger model: `absent` | `downloading` | `ready` |
    /// `failed`.
    pub tagger: String,
    /// The tagger model's own percent, while it is inbound.
    pub tagger_percent: Option<u32>,
}

impl DatasetMirror {
    /// The fraction downloaded, when the size is known.
    pub fn percent(&self) -> Option<u32> {
        let total = self.total?;
        (total > 0).then_some(((self.received.min(total) as f64 / total as f64) * 100.0) as u32)
    }

    pub fn is_ready(&self) -> bool {
        self.phase == "ready"
    }

    /// The readout under the bar: a speed, and what is left at it.
    pub fn rate(&self) -> Option<String> {
        let speed = self.speed.filter(|speed| *speed > 1.0)?;
        let per_sec = if speed >= 1_048_576.0 {
            format!("{:.1} MB/s", speed / 1_048_576.0)
        } else {
            format!("{:.0} kB/s", speed / 1024.0)
        };
        match self.eta_secs {
            Some(secs) if secs > 0 => Some(format!("{per_sec} — {secs}s left")),
            _ => Some(per_sec),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mirror(phase: &str, received: u64, total: Option<u64>) -> DatasetMirror {
        DatasetMirror {
            phase: phase.into(),
            received,
            total,
            ..Default::default()
        }
    }

    #[test]
    fn percent_needs_a_size() {
        assert_eq!(mirror("downloading", 10, None).percent(), None);
        assert_eq!(mirror("downloading", 10, Some(0)).percent(), None);
        assert_eq!(mirror("downloading", 10, Some(40)).percent(), Some(25));
    }

    #[test]
    fn a_paused_mirror_keeps_the_bytes_it_had() {
        let paused = mirror("paused", 30, Some(40));
        assert_eq!(paused.percent(), Some(75));
        assert!(!paused.is_ready());
        assert!(mirror("ready", 0, None).is_ready());
    }

    #[test]
    fn the_rate_readout_needs_a_real_speed() {
        assert_eq!(mirror("downloading", 0, None).rate(), None);
        let mut slow = mirror("downloading", 0, Some(100));
        slow.speed = Some(0.5);
        assert_eq!(slow.rate(), None, "a crawl is not an estimate");
        slow.speed = Some(512_000.0);
        assert_eq!(slow.rate().as_deref(), Some("500 kB/s"));
        slow.speed = Some(2_097_152.0);
        slow.eta_secs = Some(3);
        assert_eq!(slow.rate().as_deref(), Some("2.0 MB/s — 3s left"));
    }

    #[test]
    fn the_wire_shape_tolerates_a_backend_that_sends_less() {
        let parsed: DatasetMirror =
            serde_json::from_str(r#"{"phase":"ready","received":0,"words":1234}"#).unwrap();
        assert!(parsed.is_ready());
        assert_eq!(parsed.words, Some(1234));
        assert_eq!(parsed.total, None);
        assert_eq!(parsed.speed, None);
    }
}
