//! Study mode's row: what a script sends to turn it on and study a deck.

use mareader_study::mode::{Key, Mode};
use serde_json::Value;

use crate::command::Command;
use crate::command_spec::{CommandSpec, acts, json, reader_only, row};
use crate::session::Session;
use crate::study_mode::Action;

/// Study mode's asks a script can send.
pub(crate) const ACTS: &str = "toggle|off|start|known|undo|next_batch|retry|read_previous";

const MODES: &[(&str, Mode)] = &[("proactive", Mode::Proactive), ("reactive", Mode::Reactive)];

/// The ask, its pane, the mode a toggle turns on and a known card's word.
const PARAMS: &[(&str, &str)] = &[
    ("action", ACTS),
    ("pane", "int?"),
    ("mode", "proactive|reactive?"),
    ("word", "str?"),
];

pub(crate) const MODE: CommandSpec = row(
    "study.mode",
    "Study mode in a pane: on and off, its deck and its gate.",
    PARAMS,
    reader_only,
    parse,
);

/// A toggle without a mode turns on the one the settings pick.
fn parse(at: &Session, params: &Value) -> Result<Command, String> {
    let name = json::text(params, "action")?;
    let pane = json::pane(at, params)?;
    let action = match name.as_str() {
        "toggle" => {
            let picked = json::maybe_one_of(params, "mode", MODES)?;
            let mode = picked.unwrap_or(at.settings().study.mode.mode);
            Action::Toggle { pane, mode }
        }
        "off" => Action::Off(pane),
        "start" => Action::Start(pane),
        "known" => {
            let word = json::text(params, "word")?;
            let key = Key::parse(&word).ok_or_else(|| format!("{word:?} names no word"))?;
            Action::Known { pane, key }
        }
        "undo" => Action::Undo(pane),
        "next_batch" => Action::NextBatch(pane),
        "retry" => Action::Retry(pane),
        "read_previous" => Action::ReadPrevious(pane),
        other => return Err(acts::refused("study.mode", other, ACTS)),
    };
    Ok(Command::StudyMode(action))
}
