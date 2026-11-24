//! Study's rows: what a script sends to study a deck, and to Space's words.

use mareader_library::WordId;
use mareader_study::mode::{Key, Mode, Outcome, Pos};
use serde_json::Value;

use crate::command::Command;
use crate::command_spec::{CommandSpec, acts, anywhere, json, reader_only, row};
use crate::session::Session;
use crate::space::{Action as SpaceAction, Room};
use crate::study_mode::{Action, Ask};

/// Study mode's asks a script can send.
pub(crate) const ACTS: &str = "toggle|off|start|reopen|known|undo|next_batch|retry|use_dictionary|skip_rest|read_previous|recall";

/// What a recall asks; `open` takes `page`, `from` and `to`, `answer` `text`.
const ASKS: &str = "open|answer|again|reveal|memorized|learning|missed|close";
const ASKS_OR_NONE: &str = "open|answer|again|reveal|memorized|learning|missed|close?";

const MODES: &[(&str, Mode)] = &[("proactive", Mode::Proactive), ("reactive", Mode::Reactive)];

/// The ask, its pane, the mode a toggle turns on, a known card's word and
/// what a recall asks.
const PARAMS: &[(&str, &str)] = &[
    ("action", ACTS),
    ("pane", "int?"),
    ("mode", "proactive|reactive?"),
    ("word", "str?"),
    ("ask", ASKS_OR_NONE),
    ("page", "int?"),
    ("from", "int?"),
    ("to", "int?"),
    ("text", "str?"),
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
        "reopen" => Action::Reopen(pane),
        "known" => {
            let word = json::text(params, "word")?;
            let key = Key::parse(&word).ok_or_else(|| format!("{word:?} names no word"))?;
            Action::Known { pane, key }
        }
        "undo" => Action::Undo(pane),
        "next_batch" => Action::NextBatch(pane),
        "retry" => Action::Retry(pane),
        "use_dictionary" => Action::UseDictionary(pane),
        "skip_rest" => Action::SkipRest(pane),
        "read_previous" => Action::ReadPrevious(pane),
        "recall" => Action::Recall {
            pane,
            ask: recall(params)?,
        },
        other => return Err(acts::refused("study.mode", other, ACTS)),
    };
    Ok(Command::StudyMode(action))
}

/// A recall's ask, from `ask` and what it takes.
fn recall(params: &Value) -> Result<Ask, String> {
    let ask = match json::text(params, "ask")?.as_str() {
        "open" => {
            let page = json::count(params, "page")?;
            let from = json::count(params, "from")?;
            let chars = from..json::count(params, "to")?;
            Ask::Open { page, chars }
        }
        "answer" => Ask::Answer(json::text(params, "text")?),
        "again" => Ask::Again,
        "reveal" => Ask::Reveal,
        "memorized" => Ask::Finish(Outcome::Memorized),
        "learning" => Ask::Finish(Outcome::Learning),
        "missed" => Ask::Finish(Outcome::Missed),
        "close" => Ask::Close,
        other => return Err(acts::refused("study.mode", other, ASKS)),
    };
    Ok(ask)
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

/// The room Space shows.
const ROOM_PARAMS: &[(&str, &str)] = &[("room", "hub|words|notes")];

pub(crate) const SPACE_SHOW: CommandSpec = row(
    "space.show",
    "Shows Space at its hub, its words or its notes.",
    ROOM_PARAMS,
    anywhere,
    show_space,
);

fn show_space(_at: &Session, params: &Value) -> Result<Command, String> {
    let name = json::text(params, "room")?;
    let refused = || acts::refused("space.show", &name, "hub|words|notes");
    let room = Room::parse(&name).ok_or_else(refused)?;
    Ok(Command::ShowSpace(room))
}

/// The mark, by its number.
const MARK_PARAMS: &[(&str, &str)] = &[("mark", "int")];

pub(crate) const OPEN_MARK: CommandSpec = row(
    "reader.open-mark",
    "Opens a highlight's, note's or bookmark's book where it is.",
    MARK_PARAMS,
    anywhere,
    open_mark,
);

fn open_mark(_at: &Session, params: &Value) -> Result<Command, String> {
    let mark = json::maybe_mark(params, "mark")?;
    let mark = mark.ok_or_else(|| json::wants("mark", "a mark's number"))?;
    Ok(Command::OpenMark(mark))
}
