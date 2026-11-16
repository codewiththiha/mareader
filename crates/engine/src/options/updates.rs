//! Option keys for update settings and terminal/shot mock triggers.

use super::{field, flag};
use crate::command::Command;
use crate::session::Session;
use crate::updates::{Action, MockMode, MockUpdate, UpdatePlatform};

/// Update-related option keys; `None` when the key belongs elsewhere.
pub(super) fn set(key: &str, value: &str, at: &Session) -> Option<Result<Vec<Command>, String>> {
    let parsed = match key {
        "auto_update" => field("updates.auto_update", flag(value)),
        "check_updates" => field("updates.check_on_startup", flag(value)),
        "mock_update" => mock_option(value, at),
        "mock_update_file" => mock_file(value, at),
        _ => return None,
    };
    Some(parsed)
}

fn mock_option(value: &str, at: &Session) -> Result<Vec<Command>, String> {
    let raw = value.trim();
    if let Some((lhs, rhs)) = raw.split_once(':') {
        let platform = UpdatePlatform::parse(lhs)
            .ok_or_else(|| format!("mock_update: unknown platform {lhs:?}"))?;
        let mode = MockMode::parse(rhs);
        let version = if mode.is_some() {
            "0.0.9".to_owned()
        } else {
            rhs.to_owned()
        };
        return Ok(vec![Command::Update(Action::Mock(MockUpdate {
            version,
            notes: None,
            url: None,
            platform: Some(platform),
            mode,
        }))]);
    }
    if let Some(mode) = MockMode::parse(raw) {
        return Ok(vec![Command::Update(Action::Mock(MockUpdate {
            version: "0.0.9".to_owned(),
            notes: None,
            url: None,
            platform: Some(at.updates().platform()),
            mode: Some(mode),
        }))]);
    }
    if let Some(platform) = UpdatePlatform::parse(raw) {
        return Ok(vec![Command::Update(Action::Mock(MockUpdate {
            version: "0.0.9".to_owned(),
            notes: None,
            url: None,
            platform: Some(platform),
            mode: None,
        }))]);
    }
    let version = if raw.is_empty() || raw.eq_ignore_ascii_case("true") || raw == "1" {
        "0.0.9".to_owned()
    } else {
        raw.to_owned()
    };
    Ok(vec![Command::Update(Action::Mock(MockUpdate {
        version,
        notes: None,
        url: None,
        platform: Some(at.updates().platform()),
        mode: None,
    }))])
}

fn mock_file(path: &str, at: &Session) -> Result<Vec<Command>, String> {
    let notes = std::fs::read_to_string(path.trim())
        .map_err(|e| format!("mock_update_file: could not read {path:?}: {e}"))?;
    Ok(vec![Command::Update(Action::Mock(MockUpdate {
        version: "0.0.9".to_owned(),
        notes: Some(notes),
        url: None,
        platform: Some(at.updates().platform()),
        mode: None,
    }))])
}
