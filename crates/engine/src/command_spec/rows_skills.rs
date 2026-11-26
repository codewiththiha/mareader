//! Skills' row: what a script sends to run a skill and handle its card.

use mareader_settings::Skill;
use serde_json::Value;

use crate::command::Command;
use crate::command_spec::{CommandSpec, acts, json, reader_only, row};
use crate::session::Session;
use crate::skills::{Action, AskId};

const ACTS: &str = "run|run_page|retry|close|cancel|remove|pin|show";

/// The ask, its pane, the skill as `translate` or `custom-3`, the page
/// and spot a page run points at, and the ask a card's buttons name.
const PARAMS: &[(&str, &str)] = &[
    ("action", ACTS),
    ("pane", "int?"),
    ("skill", "str?"),
    ("page", "int?"),
    ("x", "real?"),
    ("y", "real?"),
    ("ask", "int?"),
];

pub(crate) const SKILL: CommandSpec = row(
    "ai.skill",
    "Runs a skill on the selection or a page; retries, closes or shows its card.",
    PARAMS,
    reader_only,
    parse,
);

fn parse(at: &Session, params: &Value) -> Result<Command, String> {
    let name = json::text(params, "action")?;
    let action = match name.as_str() {
        "run" => Action::Run {
            pane: json::pane(at, params)?,
            skill: skill(params)?,
        },
        "run_page" => Action::RunPage {
            pane: json::pane(at, params)?,
            page: json::count(params, "page")?,
            x: json::maybe_real(params, "x")?.unwrap_or_default(),
            y: json::maybe_real(params, "y")?.unwrap_or_default(),
            skill: skill(params)?,
        },
        "retry" => Action::Retry(ask(params)?),
        "close" => Action::Close(ask(params)?),
        "cancel" => Action::Cancel(ask(params)?),
        "remove" => Action::Remove(ask(params)?),
        "pin" => Action::Pin(ask(params)?),
        "show" => Action::Show(ask(params)?),
        other => return Err(acts::refused("ai.skill", other, ACTS)),
    };
    Ok(Command::Skill(action))
}

fn skill(params: &Value) -> Result<Skill, String> {
    let key = json::text(params, "skill")?;
    Skill::parse(&key).ok_or_else(|| format!("{key:?} names no skill"))
}

fn ask(params: &Value) -> Result<AskId, String> {
    Ok(AskId(json::wide(params, "ask")?))
}
