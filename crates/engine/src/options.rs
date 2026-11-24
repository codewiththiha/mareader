//! The headless knobs, parsed: one key, one command.
//!
//! `set_option`'s keys began as a private string channel into the front end
//! (finding F7): a shot, an example or a query string named a dial and the
//! window's own code turned it into state. What most of those keys set is the
//! engine's (a setting by its dotted path, as `Command::SetField`), so the
//! parsing lives here, where a front end, a command line and a test all meet
//! the same answer. A key that reads what is set so far reads it here, from the
//! session it arrives with; a key that reads the books asks the library, which
//! answers when the ask lands. A front end keeps the keys that name its
//! toolkit, its widgets, and the room its own window has.
//!
//! `None` is the answer for a key this module does not own, which is how a
//! front end's own keys stay reachable: it asks here first and falls through.

mod appearance;
mod library;
mod panes;
mod study;
mod updates;

use mareader_workspace::{Bounds, Edge, SplitAxis};

use crate::command::Command;
use crate::documents::OpenRequest;
use crate::session::Session;

/// The commands a key asks for, in order; `None` when it is a front end's own.
///
/// A batch, because one key can ask for two writes: `sort=title:desc` sets the
/// key and the way it runs, and the settings hold them apart. `at` is the
/// session the key arrived at, which is what a key that reads the looks as they
/// stand, or the panes as they are, reads.
pub fn parse(key: &str, value: &str, at: &Session) -> Option<Result<Vec<Command>, String>> {
    appearance::set(key, value, at)
        .or_else(|| library::set(key, value, at))
        .or_else(|| panes::set(key, value, at))
        .or_else(|| study::set(key, value, at))
        .or_else(|| updates::set(key, value, at))
}

/// Parses and applies an engine-owned option key against `session`.
///
/// Setting writes, focus changes, and fixture-sized splits land at once so
/// subsequent keys in the same option string read them; opens are aimed at
/// the active pane and returned alongside any remaining queued commands.
pub fn apply(
    session: &mut Session,
    key: &str,
    value: &str,
) -> Option<Result<Vec<Command>, String>> {
    session.reconcile_texts();
    let commands = match parse(key, value, session)? {
        Ok(commands) => commands,
        Err(why) => return Some(Err(why)),
    };
    let mut deferred = Vec::new();
    for command in commands {
        match command {
            Command::SetField { .. } | Command::FocusPane(_) => {
                let done = session.execute(command, 0.0);
                if let Some(why) = done.refused {
                    return Some(Err(why));
                }
            }
            Command::Split(axis) => {
                let pane = session
                    .tree()
                    .panes()
                    .last()
                    .copied()
                    .unwrap_or(session.active());
                let edge = match axis {
                    SplitAxis::Horizontal => Edge::Right,
                    SplitAxis::Vertical => Edge::Bottom,
                };
                let area = Bounds::new(0.0, 0.0, 1600.0, 1000.0);
                if session.split_at(pane, edge, area).is_none() {
                    return Some(Err("panes: no room for another".to_owned()));
                }
            }
            Command::Open(request) => {
                deferred.push(Command::Open(OpenRequest {
                    pane: Some(session.active()),
                    ..request
                }));
            }
            other => deferred.push(other),
        }
    }
    Some(Ok(deferred))
}

/// A setting written by its dotted path, as the session's `put` takes it.
pub(super) fn field<T: serde::Serialize>(path: &str, value: T) -> Result<Vec<Command>, String> {
    let value = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    Ok(vec![Command::SetField {
        path: path.into(),
        value,
    }])
}

/// `true`, and only `true`, is on.
pub(super) fn flag(value: &str) -> bool {
    value == "true"
}

/// A number, with the key's own hint when the value is not one.
pub(super) fn number<T: std::str::FromStr>(value: &str, hint: &str) -> Result<T, String> {
    value.parse::<T>().map_err(|_| hint.to_owned())
}

/// One of `choices` by name, ignoring case; `hint` is the refusal.
pub(super) fn named<T: Copy>(value: &str, choices: &[(&str, T)], hint: &str) -> Result<T, String> {
    let found = choices
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(value))
        .map(|(_, some)| *some);
    found.ok_or_else(|| hint.to_owned())
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
