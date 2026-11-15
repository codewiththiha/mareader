//! Panes, and the pages in them: splits, focus, opening, turning.

use mareader_settings::EdgeParking;
use mareader_workspace::{Edge, PaneId, SplitAxis};

use super::note;
use crate::command::Command;
use crate::documents::{OpenRequest, Placement};
use crate::event::{Event, PaneAction};
use crate::route::Route;
use crate::session::{Done, Session};
use crate::toast::Toast;

pub(super) fn run(session: &mut Session, intent: Command, now: f64) -> Done {
    match intent {
        Command::Split(axis) => split(session, axis),
        Command::FocusPane(id) => session.focus(id),
        Command::CyclePane { forward } => cycle(session, forward),
        Command::ClosePane(id) => {
            if session.close(id) {
                session.close_document_pane(id);
            }
        }
        Command::SetRatio(..) => return session.apply(intent),
        Command::Open(request) => open(session, request),
        Command::Export {
            name,
            format,
            contents,
        } => session.emit(Event::Export {
            name,
            format,
            contents,
        }),
        Command::CopyPages { pane, pages } => {
            if let Some(doc) = session.documents().document_of(pane) {
                session.start_page_copy(doc, pages);
            }
        }
        Command::Search { pane, query } => {
            if let Some(doc) = session.documents().place(pane).map(|p| p.doc)
                && let Some(document) = session.documents().document_of(pane)
            {
                session.ask_search(doc, &document, &query);
            }
        }
        other => pages(session, other, now),
    }
    Done::changed()
}

/// What moves the page a pane shows; most act on the focused pane.
fn pages(session: &mut Session, intent: Command, now: f64) {
    let active = session.active();
    match intent {
        Command::Zoom(zoom) => view_action(session, active, PaneAction::Zoom(zoom), now),
        Command::View(mode) => {
            if session.document_set_mode(active, mode) {
                view_action(session, active, PaneAction::SetMode(mode), now);
            }
        }
        Command::Arrow(arrow) => view_action(session, active, PaneAction::Arrow(arrow), now),
        Command::Page { forward } => page_key(session, active, forward, now),
        Command::AutoScroll => auto_scroll(session, active, now),
        Command::FirstPage => go_to(session, active, 0, now),
        Command::LastPage => go_to(session, active, usize::MAX, now),
        Command::GoTo { pane, page } => go_to(session, pane, page, now),
        Command::Turn { pane, forward } => turn(session, pane, forward, now),
        _ => {}
    }
}

fn view_action(session: &mut Session, pane: PaneId, action: PaneAction, now: f64) {
    session.emit(Event::PaneView { pane, action, now });
}

fn go_to(session: &mut Session, pane: PaneId, page: usize, now: f64) {
    if let Some(landed) = session.document_go_to(pane, page) {
        view_action(session, pane, PaneAction::GoTo(landed), now);
    }
}

fn turn(session: &mut Session, pane: PaneId, forward: bool, now: f64) {
    if let Some(landed) = session.document_turn(pane, forward) {
        view_action(session, pane, PaneAction::GoTo(landed), now);
    }
}

fn page_key(session: &mut Session, pane: PaneId, forward: bool, now: f64) {
    let paged = session
        .documents()
        .place(pane)
        .is_some_and(|place| place.mode.paged());
    if paged {
        turn(session, pane, forward, now);
    } else {
        view_action(session, pane, PaneAction::PageKey { forward }, now);
    }
}

fn auto_scroll(session: &mut Session, pane: PaneId, now: f64) {
    let scrolls = session
        .documents()
        .place(pane)
        .is_some_and(|place| !place.mode.paged());
    if scrolls {
        view_action(session, pane, PaneAction::ToggleDrift, now);
    } else {
        note(
            session,
            Toast::info("Auto-scroll runs in the scrolling views"),
        );
    }
}

/// The new pane takes the focus, as a split asks it to.
fn split(session: &mut Session, axis: SplitAxis) {
    let edge = match axis {
        SplitAxis::Horizontal => Edge::Right,
        SplitAxis::Vertical => Edge::Bottom,
    };
    let area = session.area();
    if let Some(id) = session.split_at(session.active(), edge, area) {
        session.focus(id);
    }
}

/// F6: the next pane in reading order, by way of the parked tab if shown.
fn cycle(session: &mut Session, forward: bool) {
    let tab = session.edge_tab();
    let panes = session.tree().panes();
    let active = session.active();
    let at = panes.iter().position(|p| *p == active).unwrap_or(0);
    let n = panes.len().max(1);
    let parked = session.settings().workspace.edge_parking == EdgeParking::Tab;
    let ends = (forward && at + 1 == n) || (!forward && at == 0);
    if parked && tab.shown && !tab.focused && ends {
        session.emit(Event::FocusEdgeTab);
        return;
    }
    let next = match (tab.focused, forward) {
        (true, true) => 0,
        (true, false) => n - 1,
        (false, true) => (at + 1) % n,
        (false, false) => (at + n - 1) % n,
    };
    if let Some(pane) = panes.get(next) {
        session.focus(*pane);
    }
    session.emit(Event::ReleaseFocus);
}

fn open(session: &mut Session, request: OpenRequest) {
    // Held while the library loads, to open where it was left.
    let Some(request) = session.library_resume(request) else {
        return;
    };
    go(session, Route::Reader);
    let area = session.area();
    let mut pane = request.pane.unwrap_or(session.active());
    let edge = match request.placement {
        Placement::Into => None,
        Placement::Beside => Some(session.beside(pane, area)),
        Placement::Edge(edge) => Some(edge),
    };
    if let Some(edge) = edge {
        match session.split_at(pane, edge, area) {
            Some(new) => pane = new,
            None => return,
        }
    }
    if session.tree().panes().contains(&pane) {
        session.clear_last();
        session.focus(pane);
        if let Some(toast) = session.open_pane(pane, request) {
            note(session, toast);
        }
    }
}

/// The one way between routes; the page left behind is told via an event.
pub(super) fn go(session: &mut Session, route: Route) {
    session.show(route);
}

/// The panes showing one of `books`, or a file at one of `paths`.
pub(super) fn showing(
    session: &Session,
    books: &[mareader_library::BookId],
    paths: &[std::path::PathBuf],
) -> Vec<PaneId> {
    let panes = session.tree().panes();
    let shows = |pane: &PaneId| match session.documents().book(*pane) {
        Some(book) => books.contains(&book),
        None => session
            .documents()
            .path(*pane)
            .is_some_and(|open| paths.iter().any(|p| p.as_path() == open)),
    };
    panes.into_iter().filter(shows).collect()
}
