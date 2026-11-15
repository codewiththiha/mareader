//! The JSON a script sends, read one key at a time.
//!
//! A helper answers either the value it read or one line saying what the key
//! wanted, and `Session::execute_id` puts the command's id in front of it. A
//! kind ending in `?` may be left out. `pane` left out means the pane the
//! session has focused, which is what a window sends; a field that is an
//! `Option<PaneId>` keeps its `None`, because the engine aims that one itself.

use std::path::PathBuf;

use serde_json::Value;

use mareader_appearance::Rgb;
use mareader_library::{BookId, MarkId};
use mareader_session::Anchor;
use mareader_speech::Spot;
use mareader_workspace::{Bounds, PaneId};

use crate::session::Session;

/// What a key wanted, when what it held is not that.
pub(crate) fn wants(key: &str, what: &str) -> String {
    format!("{key:?} wants {what}")
}

/// What a key that has to be there is called when it is not.
pub(crate) fn missing(key: &str) -> String {
    format!("no {key:?} in the params")
}

/// The member `key` names, when the params hold it.
pub(crate) fn member<'a>(json: &'a Value, key: &str) -> Option<&'a Value> {
    json.get(key)
}

/// The member `key` names, or the line that says it is not there.
pub(crate) fn held<'a>(json: &'a Value, key: &str) -> Result<&'a Value, String> {
    member(json, key).ok_or_else(|| missing(key))
}

/// A whole number, as a page, a count or a pane's id.
pub(crate) fn count(json: &Value, key: &str) -> Result<usize, String> {
    match held(json, key)?.as_u64() {
        Some(whole) => Ok(whole as usize),
        None => Err(wants(key, "a whole number")),
    }
}

/// A whole number that may be left out.
pub(crate) fn maybe_count(json: &Value, key: &str) -> Result<Option<usize>, String> {
    match member(json, key) {
        None => Ok(None),
        Some(_) => Ok(Some(count(json, key)?)),
    }
}

/// A pane's number, as a window holds it.
pub(crate) fn wide(json: &Value, key: &str) -> Result<u64, String> {
    held(json, key)?
        .as_u64()
        .ok_or_else(|| wants(key, "a pane's number"))
}

/// An optional whole `u64` (such as a size floor in bytes).
pub(crate) fn maybe_wide(json: &Value, key: &str) -> Result<Option<u64>, String> {
    match member(json, key) {
        None => Ok(None),
        Some(v) => match v.as_u64() {
            Some(n) => Ok(Some(n)),
            None => Err(wants(key, "a whole number")),
        },
    }
}

/// A book's number, which is wider than a pane's.
pub(crate) fn book(json: &Value, key: &str) -> Result<BookId, String> {
    one_book(held(json, key)?).ok_or_else(|| wants(key, "a book's number"))
}

/// An optional mark's number, as digits or an integer.
pub(crate) fn maybe_mark(json: &Value, key: &str) -> Result<Option<MarkId>, String> {
    match member(json, key) {
        None => Ok(None),
        Some(v) => match one_id(v) {
            Some(id) => Ok(Some(MarkId(id))),
            None => Err(wants(key, "a mark's number")),
        },
    }
}

/// A book's number as a script sends it: a number, or its digits as text.
fn one_book(found: &Value) -> Option<BookId> {
    one_id(found).map(BookId)
}

/// A 128-bit id as a number or its decimal digits.
fn one_id(found: &Value) -> Option<u128> {
    match found {
        Value::String(text) => text.parse::<u128>().ok(),
        _ => found.as_u64().map(u128::from),
    }
}

/// A whole number as a flag, or a flag as one: `true` and `1` both say yes.
pub(crate) fn flag(json: &Value, key: &str) -> Result<bool, String> {
    match held(json, key)? {
        Value::Bool(flag) => Ok(*flag),
        Value::Number(number) => Ok(number.as_u64().is_some_and(|n| n != 0)),
        _ => Err(wants(key, "true or false")),
    }
}

/// A flag that may be left out, and is then off.
pub(crate) fn off_unless(json: &Value, key: &str) -> Result<bool, String> {
    match member(json, key) {
        None => Ok(false),
        Some(_) => flag(json, key),
    }
}

/// A flag that may be left out, and stays `None` when it is.
pub(crate) fn maybe_flag(json: &Value, key: &str) -> Result<Option<bool>, String> {
    member(json, key).map(|_| flag(json, key)).transpose()
}

/// Text, as a name, a word or a path.
pub(crate) fn text(json: &Value, key: &str) -> Result<String, String> {
    held(json, key)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| wants(key, "text"))
}

/// Text that may be left out.
pub(crate) fn maybe_text(json: &Value, key: &str) -> Result<Option<String>, String> {
    member(json, key).map(|_| text(json, key)).transpose()
}

/// A path, which a script gives as text.
pub(crate) fn path(json: &Value, key: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(text(json, key)?))
}

/// A number with a fraction, as a ratio or a dial.
pub(crate) fn real(json: &Value, key: &str) -> Result<f32, String> {
    held(json, key)?
        .as_f64()
        .map(|found| found as f32)
        .ok_or_else(|| wants(key, "a number"))
}

/// A number with a fraction that may be left out.
pub(crate) fn maybe_real(json: &Value, key: &str) -> Result<Option<f32>, String> {
    member(json, key).map(|_| real(json, key)).transpose()
}

/// The pane a command means: the one named, else the one in focus.
pub(crate) fn pane(at: &Session, json: &Value) -> Result<PaneId, String> {
    named_pane(at, json, "pane")
}

/// The pane `key` names, else the one in focus.
pub(crate) fn named_pane(at: &Session, json: &Value, key: &str) -> Result<PaneId, String> {
    match member(json, key) {
        None => Ok(at.active()),
        Some(_) => Ok(PaneId(wide(json, key)?)),
    }
}

/// A pane a command holds as an `Option`: named, or left to the engine.
pub(crate) fn maybe_pane(json: &Value, key: &str) -> Result<Option<PaneId>, String> {
    member(json, key)
        .map(|_| wide(json, key).map(PaneId))
        .transpose()
}

/// A list of whole numbers, as the pages a word list takes.
pub(crate) fn counts(json: &Value, key: &str) -> Result<Vec<usize>, String> {
    let list = held(json, key)?
        .as_array()
        .ok_or_else(|| wants(key, "a list of numbers"))?;
    list.iter()
        .map(|one| {
            one.as_u64()
                .map(|whole| whole as usize)
                .ok_or_else(|| wants(key, "a list of whole numbers"))
        })
        .collect()
}

/// A list of whole numbers that may be left out, and is then empty.
pub(crate) fn counts_or_none(json: &Value, key: &str) -> Result<Vec<usize>, String> {
    match member(json, key) {
        None => Ok(Vec::new()),
        Some(_) => counts(json, key),
    }
}

/// A list of flags, as the seam a divider drag names.
pub(crate) fn flags(json: &Value, key: &str) -> Result<Vec<bool>, String> {
    let list = held(json, key)?
        .as_array()
        .ok_or_else(|| wants(key, "a list of flags"))?;
    list.iter()
        .map(|one| {
            one.as_bool()
                .ok_or_else(|| wants(key, "a list of true and false"))
        })
        .collect()
}

/// A list of paths, as the books a location went from.
pub(crate) fn paths(json: &Value, key: &str) -> Result<Vec<PathBuf>, String> {
    let list = held(json, key)?
        .as_array()
        .ok_or_else(|| wants(key, "a list of paths"))?;
    list.iter()
        .map(|one| {
            one.as_str()
                .map(PathBuf::from)
                .ok_or_else(|| wants(key, "a list of paths"))
        })
        .collect()
}

/// A list of paths that may be left out, and is then empty.
pub(crate) fn paths_or_none(json: &Value, key: &str) -> Result<Vec<PathBuf>, String> {
    match member(json, key) {
        None => Ok(Vec::new()),
        Some(_) => paths(json, key),
    }
}

/// What a list of book numbers is called in a refusal.
const BOOKS: &str = "a list of book numbers";

/// A list of book numbers that may be left out, and is then empty.
pub(crate) fn books_or_none(json: &Value, key: &str) -> Result<Vec<BookId>, String> {
    let Some(found) = member(json, key) else {
        return Ok(Vec::new());
    };
    let list = found.as_array().ok_or_else(|| wants(key, BOOKS))?;
    list.iter()
        .map(|one| one_book(one).ok_or_else(|| wants(key, BOOKS)))
        .collect()
}

/// A pane's room on screen, as the four numbers a window measures.
pub(crate) fn area(json: &Value, key: &str) -> Result<Bounds, String> {
    let found = held(json, key)?;
    let part = |name: &str| -> Result<f32, String> {
        found
            .get(name)
            .ok_or_else(|| wants(key, "x, y, w and h"))?
            .as_f64()
            .map(|real| real as f32)
            .ok_or_else(|| wants(key, "x, y, w and h as numbers"))
    };
    Ok(Bounds::new(part("x")?, part("y")?, part("w")?, part("h")?))
}

/// A colour, as the three bytes a picker hands over.
pub(crate) fn rgb(json: &Value, key: &str) -> Result<Option<Rgb>, String> {
    let Some(found) = member(json, key) else {
        return Ok(None);
    };
    let list = found
        .as_array()
        .ok_or_else(|| wants(key, "three numbers, red then green then blue"))?;
    let byte = |at: usize| -> Result<u8, String> {
        list.get(at)
            .and_then(Value::as_u64)
            .and_then(|whole| u8::try_from(whole).ok())
            .ok_or_else(|| wants(key, "three numbers of a byte each"))
    };
    Ok(Some(Rgb(byte(0)?, byte(1)?, byte(2)?)))
}

/// A place in a script, which is a page and a character in it.
pub(crate) fn spot(json: &Value, key: &str) -> Result<Option<Spot>, String> {
    let Some(found) = member(json, key) else {
        return Ok(None);
    };
    let part = |name: &str| -> Result<usize, String> {
        found
            .get(name)
            .and_then(Value::as_u64)
            .map(|whole| whole as usize)
            .ok_or_else(|| wants(key, "a page and a char"))
    };
    Ok(Some(Spot {
        page: part("page")?,
        char: part("char")?,
    }))
}

/// A block and an offset into its text: where a book was left.
pub(crate) fn anchor(found: &Value) -> Result<Anchor, String> {
    let block = count(found, "block")?;
    let offset = count(found, "offset")?;
    Ok(Anchor { block, offset })
}

/// A shape of its own, read by serde where the type already has one.
pub(crate) fn shape<T: serde::de::DeserializeOwned>(
    json: &Value,
    key: &str,
    what: &str,
) -> Result<T, String> {
    let found = held(json, key)?;
    serde_json::from_value(found.clone()).map_err(|why| format!("{key:?} wants {what}: {why}"))
}

/// One of a table's names, as an enum a script spells out.
pub(crate) fn one_of<T: Clone>(json: &Value, key: &str, table: &[(&str, T)]) -> Result<T, String> {
    let name = text(json, key)?;
    table
        .iter()
        .find(|(spelled, _)| *spelled == name)
        .map(|(_, made)| made.clone())
        .ok_or_else(|| wants(key, &spellings(table)))
}

/// One of a table's names that may be left out, and is then `None`.
pub(crate) fn maybe_one_of<T: Clone>(
    json: &Value,
    key: &str,
    table: &[(&str, T)],
) -> Result<Option<T>, String> {
    match member(json, key) {
        None => Ok(None),
        Some(_) => one_of(json, key, table).map(Some),
    }
}

/// A table's names as one line: what a param's kind says, and what a refusal
/// offers instead of the name a script got wrong.
pub(crate) fn spellings<T>(table: &[(&str, T)]) -> String {
    let names: Vec<&str> = table.iter().map(|(spelled, _)| *spelled).collect();
    names.join(" or ")
}

/// The object `key` names, for the params that are a shape of their own.
pub(crate) fn object<'a>(json: &'a Value, key: &str) -> Result<&'a Value, String> {
    let found = held(json, key)?;
    match found.is_object() {
        true => Ok(found),
        false => Err(wants(key, "an object")),
    }
}

/// The object `key` names, when it is there.
pub(crate) fn maybe_object<'a>(json: &'a Value, key: &str) -> Option<&'a Value> {
    member(json, key).filter(|found| found.is_object())
}
