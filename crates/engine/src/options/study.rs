//! The study keys that write a setting, and the dataset the shading reads.
//!
//! What a front end keeps is its own: a staged selection, the dictionary's
//! window and page, which task a word arrives by, and the dictionary packs and
//! AI service it attaches for itself.

use super::field;
use crate::command::Command;
use crate::session::Session;

/// The study keys; `None` for a key that is not one of them.
pub(super) fn set(key: &str, value: &str, at: &Session) -> Option<Result<Vec<Command>, String>> {
    let parsed = match key {
        "meaning" => meaning(value),
        "provider" => provider(value, at),
        #[cfg(not(target_arch = "wasm32"))]
        "cefr" => cefr(value),
        _ => return None,
    };
    Some(parsed)
}

/// The meaning's language by tag, e.g. `my`.
fn meaning(value: &str) -> Result<Vec<Command>, String> {
    mareader_settings::language_name(value).ok_or("meaning: a tag, e.g. my")?;
    field("ai.tasks.explain.language", Some(value.to_owned()))
}

/// The word levels' dataset, with the shading turned on.
///
/// Two commands: the folder, which the window's own controller reads, and the
/// dial that lights the harder words up.
#[cfg(not(target_arch = "wasm32"))]
fn cefr(value: &str) -> Result<Vec<Command>, String> {
    let root = std::path::PathBuf::from(value);
    let mut found = vec![Command::AttachVocab(root)];
    found.extend(field("study.cefr.enabled", true)?);
    Ok(found)
}

/// One preset for a provider, with the explain task pointed at it.
fn provider(value: &str, at: &Session) -> Result<Vec<Command>, String> {
    let name = serde_json::Value::String(value.to_owned());
    let found = serde_json::from_value(name).map_err(|_| "provider: e.g. openai or ollama")?;
    let mut ai = at.settings().ai.clone();
    ai.presets.clear();
    let id = ai.add_preset(found, "");
    ai.tasks.explain.preset = Some(id);
    field("ai", ai)
}
