//! The library's books as panes take them.

use mareader_library::{BookId, MarkId, Spot};
use mareader_session::Anchor;
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
            let placement = clicked(session);
            open_book(session, book, (None, placement), None, now);
        }
        Command::OpenMark(mark) => open_mark(session, mark, now),
        Command::Library(Action::OpenAiSelection(id)) => {
            if let Some(item) = session.library_model().ai_selection(id) {
                let (book, spot) = (item.book.value, item.spot);
                let placement = clicked(session);
                open_book(session, book, (None, placement), Some(spot), now);
            }
        }
        Command::PlaceBook {
            book,
            pane,
            placement,
        } => open_book(session, book, (pane, placement), None, now),
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

/// Where a click opens a book; Do nothing is for the rail, as the pages
/// have nothing to drag to.
fn clicked(session: &Session) -> Placement {
    match session.settings().workspace.library_click {
        LibraryClick::Split => Placement::Beside,
        LibraryClick::Replace | LibraryClick::DragOnly => Placement::Into,
    }
}

/// Opens a mark's book at the mark, as a click opens a book.
fn open_mark(session: &mut Session, id: MarkId, now: f64) {
    let Some(mark) = session.library_model().mark(id) else {
        note(
            session,
            crate::toast::Toast::error("That note was removed."),
        );
        return;
    };
    let (book, spot) = (mark.book.value, mark.spot);
    let placement = clicked(session);
    open_book(session, book, (None, placement), Some(spot), now);
}

/// A book's request moved to `spot`: its page, else its text's place.
fn at_spot(request: OpenRequest, spot: Spot) -> OpenRequest {
    let (page, place) = match spot {
        Spot::Page { page, .. }
        | Spot::Chars {
            from: [page, _], ..
        } => (page, None),
        Spot::Text { block, start, .. } => (
            0,
            Some(Anchor {
                block,
                offset: start,
            }),
        ),
        Spot::Blocks {
            from: [block, offset],
            ..
        } => (0, Some(Anchor { block, offset })),
    };
    OpenRequest {
        page,
        place,
        spine: None,
        ..request
    }
}

/// Opens a library book, at `at` if given: shown already, a click focuses
/// its pane and turns to `at` there.
fn open_book(
    session: &mut Session,
    id: BookId,
    (pane, placement): (Option<PaneId>, Placement),
    at: Option<Spot>,
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
        let doc = session.documents().document_of(showing);
        let page = doc
            .zip(at)
            .and_then(|(d, spot)| crate::notes_panel::page_of(&d, spot));
        if let Some(page) = page {
            session.execute(
                Command::GoTo {
                    pane: showing,
                    page,
                },
                now,
            );
        }
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
            let request = match at {
                Some(spot) => at_spot(request, spot),
                None => request,
            };
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
