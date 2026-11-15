//! The forms a script sends for the panes: what a window lays out.

use serde_json::Value;

use crate::command::Command;
use crate::command_spec::{json, names};
use crate::session::Session;

/// `pane.split`: which way the new seam runs.
pub(crate) fn split(_at: &Session, params: &Value) -> Result<Command, String> {
    let axis = json::one_of(params, "axis", names::AXIS)?;
    Ok(Command::Split(axis))
}

/// `pane.focus`: the pane the keys go to.
pub(crate) fn focus(at: &Session, params: &Value) -> Result<Command, String> {
    Ok(Command::FocusPane(json::pane(at, params)?))
}

/// `pane.close`: a pane, and the book it showed.
pub(crate) fn close(at: &Session, params: &Value) -> Result<Command, String> {
    Ok(Command::ClosePane(json::pane(at, params)?))
}

/// `pane.cycle`: which way the focus moves.
pub(crate) fn cycle(_at: &Session, params: &Value) -> Result<Command, String> {
    let forward = json::flag(params, "forward")?;
    Ok(Command::CyclePane { forward })
}

/// `pane.swap`: the pane in focus and the one it trades with.
pub(crate) fn swap(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let other = json::named_pane(at, params, "other")?;
    Ok(Command::SwapPanes { pane, other })
}

/// `pane.move`: an edge to land on, in the room the window has.
pub(crate) fn move_pane(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let other = json::named_pane(at, params, "other")?;
    let edge = json::one_of(params, "edge", names::EDGE)?;
    let area = json::area(params, "area")?;
    Ok(Command::MovePane {
        pane,
        other,
        edge,
        area,
    })
}

/// `pane.ratio`: the seam a drag moves, and how far along it.
pub(crate) fn ratio(_at: &Session, params: &Value) -> Result<Command, String> {
    let path = json::flags(params, "path")?;
    let at = json::real(params, "ratio")?;
    Ok(Command::SetRatio(path, at))
}

/// `pane.view`: how many pages the pane shows.
pub(crate) fn view(_at: &Session, params: &Value) -> Result<Command, String> {
    let mode = json::one_of(params, "mode", names::VIEW_MODE)?;
    Ok(Command::View(mode))
}

/// `pane.zoom`: a step of the ladder, or what the page fits.
pub(crate) fn zoom(_at: &Session, params: &Value) -> Result<Command, String> {
    let zoom = json::one_of(params, "zoom", names::ZOOM)?;
    Ok(Command::Zoom(zoom))
}

/// `route.show`: which page fills the window.
pub(crate) fn show_route(_at: &Session, params: &Value) -> Result<Command, String> {
    let route = json::one_of(params, "route", names::ROUTE)?;
    Ok(Command::ShowRoute(route))
}
