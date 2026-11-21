//! Study's rows: what a script sends to study a deck, and to Space's words.

use mareader_library::WordId;
use mareader_study::mode::{Key, Mode, Pos};
use serde_json::Value;

use crate::command::Command;
use crate::command_spec::{CommandSpec, acts, anywhere, json, reader_only, row};
use crate::session::Session;
use crate::space::Action as SpaceAction;
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

/// What Space asks of a word a script names.
const SPACE_ACTS: &str = "memorize|unmemorize|forget";

/// The ask, and the word as `lemma|class`.
const SPACE_PARAMS: &[(&str, &str)] = &[("action", SPACE_ACTS), ("word", "str")];

pub(crate) const SPACE: CommandSpec = row(
    "space.word",
    "Memorizes, un-memorizes or forgets a studied word in every book.",
    SPACE_PARAMS,
    anywhere,
    space,
);

/// A word with no class is named by its bare lemma.
fn space(_at: &Session, params: &Value) -> Result<Command, String> {
    let word = json::text(params, "word")?;
    let key = Key::parse(&word).ok_or_else(|| format!("{word:?} names no word"))?;
    let ids = vec![WordId::of(&key.lemma, key.pos.map(Pos::name))];
    let action = match json::text(params, "action")?.as_str() {
        "memorize" => SpaceAction::Memorize(ids),
        "unmemorize" => SpaceAction::Unmemorize(ids),
        "forget" => SpaceAction::Forget(ids),
        other => return Err(acts::refused("space.word", other, SPACE_ACTS)),
    };
    Ok(Command::Space(action))
}
