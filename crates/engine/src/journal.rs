//! Bounded command journal with coalescing and deterministic replay.
//!
//! Every journaled command executed against a [`Session`] appends an
//! [`Entry`]. Consecutive commands carrying the same `coalesce` key update the
//! last entry in place, so continuous drags collapse into one replayable step.
//!
//! [`Session`]: crate::session::Session

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Maximum entries retained in memory before the oldest are dropped.
pub const MAX_ENTRIES: usize = 512;

/// Maximum nesting depth allowed for `command.batch`.
pub const MAX_BATCH_DEPTH: u32 = 4;

/// Maximum commands allowed in one `command.batch` call.
pub const MAX_BATCH_LEN: usize = 128;

/// One recorded command execution in the session journal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub seq: u64,
    pub id: String,
    pub params: Value,
    pub time: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coalesce: Option<String>,
}

/// One step in a batch or replay script.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    #[serde(default = "empty_object")]
    pub params: Value,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

/// Ring-bounded log of executed commands for inspection and replay.
#[derive(Clone, Debug)]
pub struct Journal {
    entries: VecDeque<Entry>,
    next_seq: u64,
    replaying: bool,
    limit: usize,
}

impl Default for Journal {
    fn default() -> Self {
        Self::with_limit(MAX_ENTRIES)
    }
}

impl Journal {
    /// Creates an empty journal retaining at most `limit` entries.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            next_seq: 1,
            replaying: false,
            limit: limit.max(1),
        }
    }

    /// Records a command unless replay is currently active.
    pub fn record(&mut self, id: &str, params: Value, time: f64, coalesce: Option<&str>) {
        if self.replaying {
            return;
        }
        let key = coalesce.filter(|k| !k.is_empty());
        if let Some(key) = key
            && let Some(last) = self.entries.back_mut()
            && last.id == id
            && last.coalesce.as_deref() == Some(key)
        {
            last.params = params;
            last.time = time;
            return;
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        if self.entries.len() >= self.limit {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            seq,
            id: id.to_owned(),
            params,
            time,
            coalesce: key.map(str::to_owned),
        });
    }

    /// Returns a snapshot of all retained entries, oldest first.
    pub fn entries(&self) -> Vec<Entry> {
        self.entries.iter().cloned().collect()
    }

    /// Number of retained entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the journal has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clears all retained entries without resetting sequence numbers.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Whether a replay is currently running.
    pub fn is_replaying(&self) -> bool {
        self.replaying
    }

    /// Marks replay active or inactive so replayed commands do not re-record.
    pub(crate) fn set_replaying(&mut self, active: bool) {
        self.replaying = active;
    }

    /// Converts all entries into replayable [`Step`] values.
    pub fn steps(&self) -> Vec<Step> {
        self.entries
            .iter()
            .map(|e| Step {
                id: e.id.clone(),
                params: e.params.clone(),
            })
            .collect()
    }

    /// Encodes all entries as newline-delimited JSON steps.
    pub fn to_jsonl(&self) -> String {
        let mut out = String::new();
        for entry in &self.entries {
            let step = Step {
                id: entry.id.clone(),
                params: entry.params.clone(),
            };
            if let Ok(line) = serde_json::to_string(&step) {
                out.push_str(&line);
                out.push('\n');
            }
        }
        out
    }

    /// Parses newline-delimited JSON steps for replay.
    pub fn from_jsonl(text: &str) -> Result<Vec<Step>, String> {
        let mut out = Vec::new();
        for (idx, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            let n = idx + 1;
            let step = serde_json::from_str(line).map_err(|e| format!("line {n}: {e}"))?;
            out.push(step);
        }
        Ok(out)
    }
}

/// Splits an optional `"coalesce"` key from a JSON parameter object.
pub(crate) fn split_coalesce(params: Value) -> Result<(Value, Option<String>), String> {
    let Value::Object(mut map) = params else {
        return Ok((params, None));
    };
    let key = match map.remove("coalesce") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s),
        Some(_) => return Err("\"coalesce\" wants text".to_owned()),
    };
    Ok((Value::Object(map), key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coalesces_consecutive_same_key_and_bounds_capacity() {
        let mut j = Journal::with_limit(2);
        j.record("settings.ink", json!({"ink": 50.0}), 1.0, Some("ink"));
        j.record("settings.ink", json!({"ink": 75.0}), 2.0, Some("ink"));
        assert_eq!(j.len(), 1);
        let first = j.entries();
        assert_eq!(first.first().map(|e| e.seq), Some(1));
        assert_eq!(
            first.first().map(|e| &e.params),
            Some(&json!({"ink": 75.0}))
        );

        j.record("page.first", json!({}), 3.0, None);
        j.record("page.last", json!({}), 4.0, None);
        assert_eq!(j.len(), 2);
        let kept = j.entries();
        let ids: Vec<&str> = kept.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["page.first", "page.last"]);
    }

    #[test]
    fn skips_recording_during_replay_and_roundtrips_jsonl() {
        let mut j = Journal::default();
        j.record("page.first", json!({}), 1.0, None);
        j.set_replaying(true);
        j.record("page.last", json!({}), 2.0, None);
        j.set_replaying(false);
        assert_eq!(j.len(), 1);

        let text = j.to_jsonl();
        assert_eq!(
            Journal::from_jsonl(&text),
            Ok(vec![Step {
                id: "page.first".to_owned(),
                params: json!({}),
            }])
        );
    }

    #[test]
    fn split_coalesce_strips_key_and_rejects_non_string() {
        let raw = json!({"ink": 40.0, "coalesce": "drag"});
        assert_eq!(
            split_coalesce(raw),
            Ok((json!({"ink": 40.0}), Some("drag".to_owned())))
        );
        assert!(split_coalesce(json!({"coalesce": 123})).is_err());
    }
}
