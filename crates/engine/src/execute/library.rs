//! The library's books as panes take them.

use mareader_library::BookId;
use mareader_settings::LibraryClick;
use mareader_workspace::PaneId;

use super::note;
use super::panes::{go, showing};
use crate::command::Command;
use crate::documents::{OpenRequest, Placement};
use crate::event::Event;
use crate::library::{Action, BookAction, Out, SearchAction, ShelfAction};
use crate::route::Route;
use crate::session::Session;

pub(super) fn run(session: &mut Session, intent: Command, now: f64) {
    match intent {
        Command::OpenBook(book) => {
            // Do nothing is for the rail; the page has nothing to drag to.
            let click = session.settings().workspace.library_click;
            let placement = match click {
                LibraryClick::Split => Placement::Beside,
                LibraryClick::Replace | LibraryClick::DragOnly => Placement::Into,
            };
            open_book(session, book, None, placement, now);
        }
        Command::PlaceBook {
            book,
            pane,
            placement,
        } => open_book(session, book, pane, placement, now),
        Command::Library(Action::Book(BookAction::Reveal(book))) => {
            session.library_reveal_book(book);
        }
        Command::Library(Action::Shelf(ShelfAction::Reveal(shelf))) => {
            session.library_reveal_shelf(shelf);
        }
        Command::Library(action) => library(session, action, now),
        Command::ForgetPositions { books, paths } => {
            let panes = showing(session, &books, &paths);
            for toast in session.restart_documents_at_start(&panes) {
                note(session, toast);
            }
        }
        _ => {}
    }
}

/// Opens a library book: shown already, a click focuses its pane.
fn open_book(
    session: &mut Session,
    id: BookId,
    pane: Option<PaneId>,
    placement: Placement,
    now: f64,
) {
    // A lost book asks where it went instead of failing.
    if session
        .library_model()
        .book(id)
        .is_some_and(|b| b.missing())
    {
        session.execute(Command::Library(Action::FindAgain(id)), now);
        return;
    }
    let dropped = pane.is_some();
    let panes = session.tree().panes();
    let showing = panes.into_iter().find(|&p| session.book_in(p) == Some(id));
    if let (false, Some(showing)) = (dropped, showing) {
        go(session, Route::Reader);
        session.focus(showing);
        return;
    }
    // A library book beside an empty pane goes in it, as in mareader.
    let target = pane.unwrap_or(session.active());
    let empty = session.documents().title(target).is_none();
    let placement = match placement {
        Placement::Beside if empty => Placement::Into,
        other => other,
    };
    match session.library_open(id) {
        Ok(request) => {
            let request = OpenRequest {
                pane,
                placement,
                ..request
            };
            session.execute(Command::Open(request), now);
        }
        Err(toast) => note(session, toast),
    }
}

fn library(session: &mut Session, action: Action, now: f64) {
    match action {
        Action::Search(SearchAction::Choose(_) | SearchAction::ChooseLit) => {
            session.emit(Event::Library(action.clone()));
            run_library_act(session, action, now);
        }
        Action::ApplyOrganized | Action::DiscardOrganized => {
            session.library_set_paused(None);
            run_library_act(session, action.clone(), now);
            session.emit(Event::Library(action));
        }
        other if needs_front(&other) => session.emit(Event::Library(other)),
        other => run_library_act(session, other, now),
    }
}

fn run_library_act(session: &mut Session, action: Action, now: f64) {
    for ask in session.library_act(action, now) {
        match ask {
            Out::Toast(toast) => note(session, toast),
            Out::Command(command) => {
                session.execute(command, now);
            }
        }
    }
}

/// Actions needing search focus, popups, or the AI organizer.
fn needs_front(action: &Action) -> bool {
    matches!(
        action,
        Action::Search(SearchAction::Focus)
            | Action::ShowAdd
            | Action::Organize(..)
            | Action::OrganizeAnew(..)
            | Action::ResumeOrganizing
            | Action::StopOrganizing
            | Action::DiscardPaused
            | Action::RestorePaused
    )
}
