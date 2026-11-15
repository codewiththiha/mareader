//! The window's keys that reach the engine: a file to open, how panes share.
//!
//! What stays in a front end names its own widgets: the platform skin, the
//! sidebar's panel, the bar's query, a toast, the workers' inline flag. The
//! room a split needs stays there too, and the door that carries one out.

use mareader_settings::LibraryClick;
use mareader_workspace::{MAX_PANES, SplitAxis};

use super::{field, flag, named, number};
use crate::command::Command;
use crate::documents::OpenRequest;
use crate::session::Session;

/// The panes' keys; `None` for a key that is not one of them.
pub(super) fn set(key: &str, value: &str, at: &Session) -> Option<Result<Vec<Command>, String>> {
    let parsed = match key {
        "pdf" | "epub" => open(key, value),
        "independent" => field("workspace.independent_themes", flag(value)),
        "independent_text" => field("workspace.independent_text", flag(value)),
        "independent_textures" => field("workspace.independent_textures", flag(value)),
        "shared_base" => field("workspace.shared_base_mode", flag(value)),
        "library_click" => click(value),
        "focus" => focus(value, at),
        "panes" => panes(value, at),
        _ => return None,
    };
    Some(parsed)
}

/// A sample built here, or the file the value names.
///
/// The pane is left unset: which one is in front is a front end's to know, and
/// it aims the request when the key arrives.
fn open(key: &str, value: &str) -> Result<Vec<Command>, String> {
    let (kind, pages) = value.split_once(':').unwrap_or((value, ""));
    let count = pages.parse().ok();
    let made = match (key, kind) {
        ("epub", "sample") => OpenRequest::bytes(
            mareader_epub::sample::epub(count.unwrap_or(3), "A Mareader Book"),
            "sample.epub",
        ),
        (_, "sample") => OpenRequest::bytes(
            mareader_pdf::sample::pdf(count.unwrap_or(120), "A Mareader Sample"),
            "sample.pdf",
        ),
        (_, "figures") => OpenRequest::bytes(
            mareader_pdf::sample::figures(count.unwrap_or(120), "A Mareader Sample"),
            "figures.pdf",
        ),
        (_, "scan") => OpenRequest::bytes(
            mareader_pdf::sample::scan(count.unwrap_or(120), "A Mareader Scan"),
            "scan.pdf",
        ),
        _ => OpenRequest::path(value.into()),
    };
    Ok(vec![Command::Open(made)])
}

/// The pane a shot counts: the first is 1, in the tree's own order.
fn focus(value: &str, at: &Session) -> Result<Vec<Command>, String> {
    let n: u32 = number(value, "focus: a pane number")?;
    let nth = n.saturating_sub(1) as usize;
    let pane = at.tree().panes().get(nth).copied();
    let pane = pane.ok_or("focus: no such pane")?;
    Ok(vec![Command::FocusPane(pane)])
}

/// The panes a shot asks for: one split each, alternating as the tree grows.
fn panes(value: &str, at: &Session) -> Result<Vec<Command>, String> {
    let asked: usize = number(value, "panes: 1 to 4")?;
    let want = asked.min(MAX_PANES);
    let mut made = Vec::new();
    let mut held = at.tree().len();
    while held < want {
        let axis = if held % 2 == 1 {
            SplitAxis::Horizontal
        } else {
            SplitAxis::Vertical
        };
        made.push(Command::Split(axis));
        held += 1;
    }
    Ok(made)
}

fn click(value: &str) -> Result<Vec<Command>, String> {
    let hint = "library_click: replace, split or drag_only";
    named(
        value,
        &[
            ("replace", LibraryClick::Replace),
            ("split", LibraryClick::Split),
            ("drag_only", LibraryClick::DragOnly),
        ],
        hint,
    )
    .and_then(|some| field("workspace.library_click", some))
}
