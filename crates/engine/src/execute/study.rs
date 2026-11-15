//! Study tools: marks, the word card, word lists and the dictionaries.

use mareader_appearance::Rgb;
use mareader_library::{MarkKind, Pen};
use mareader_workspace::PaneId;

use crate::command::Command;
use crate::dict::Action as DictAction;
use crate::event::Event;
use crate::library::wall_ms;
use crate::notes_panel::{bookmark_label, bookmark_spot, page_of};
use crate::session::{Done, Session};
use crate::study::state::{Bookmarking, Highlighted};

pub(super) fn run(session: &mut Session, intent: Command) -> Done {
    match intent {
        Command::WordList { pane, pages } => {
            let list = session.settings().study.words.instant().clone();
            if let Some(ask) = session.word_ask(pane, pages, list) {
                session.start_word_list(ask);
            }
        }
        Command::WordListWindow { pane, pages } => {
            let list = session.settings().study.words.last.clone();
            session.emit(Event::WordListWindow { pane, pages, list });
        }
        Command::Study(action) => study(session, action),
        Command::Explain(pane) => session.emit(Event::Explain(pane)),
        Command::LookUp(pane) => session.emit(Event::LookUp(pane)),
        Command::Highlight { pane, pen, color } => highlight(session, pane, pen, color),
        Command::Note(pane) => session.emit(Event::Note(pane)),
        Command::Bookmark(pane) => bookmark(session, pane.unwrap_or(session.active())),
        Command::ClearSelection(pane) => {
            if session.clear_document_selection(pane) {
                return Done::changed();
            }
            return Done::quiet();
        }
        Command::SelectText { pane, page, text } => {
            session.select_document_text(pane, page, &text);
        }
        Command::Dict(DictAction::Window(window)) => session.emit(Event::DictWindow(window)),
        Command::Dict(DictAction::Pursue(word)) => session.emit(Event::Pursue(word)),
        Command::Dict(action) if !session.dict_act_pack(&action) => {
            session.emit(Event::Dict(action));
        }
        _ => {}
    }
    Done::changed()
}

fn study(session: &mut Session, action: crate::study::Action) {
    use crate::study::Action;
    match action {
        Action::Remove(ids) => {
            session.study_remove_marks(ids.clone());
            session.emit(Event::Study(Action::Remove(ids)));
        }
        Action::Restore(ids) => session.study_restore_marks(&ids),
        Action::Restyle(id, style) => {
            session.study_restyle(id, style);
            session.emit(Event::Study(Action::Restyle(id, style)));
        }
        Action::RestyleAll(changes, skipped) => {
            session.study_restyle_all(changes, skipped);
        }
        Action::Unstyle(looks) => session.study_unstyle(&looks),
        Action::DeleteNote(id) => {
            session.study_forget_note(id);
            session.emit(Event::Study(Action::DeleteNote(id)));
        }
        Action::RestoreNote(id, text) => session.study_restore_note(id, text),
        Action::SetNote(id, text) => session.study_set_note(id, text),
        Action::DropMarks(ids) => session.study_drop_marks(&ids),
        other => session.emit(Event::Study(other)),
    }
}

fn highlight(session: &mut Session, pane: PaneId, pen: Pen, color: Option<Rgb>) {
    let Some((spot, text)) = session.documents().highlighted(pane) else {
        return;
    };
    session.clear_document_selection(pane);
    let book = session.book_in(pane);
    let name = session
        .documents()
        .title(pane)
        .unwrap_or_default()
        .to_owned();
    let at = Highlighted {
        book,
        name: &name,
        text,
        spot,
        pen,
        color,
    };
    session.study_highlight(at, wall_ms());
}

fn bookmark(session: &mut Session, pane: PaneId) {
    let Some(doc) = session.documents().document_of(pane) else {
        return;
    };
    let Some(page) = session.documents().current(pane) else {
        return;
    };
    let book = session.book_in(pane);
    let name = session
        .documents()
        .title(pane)
        .unwrap_or_default()
        .to_owned();
    let spot = bookmark_spot(&doc, page);
    // A result's later pages share its first page's bookmark.
    let here = page_of(&doc, spot).unwrap_or(page);
    let (lib, id) = session.study().lib_of(session.library_model(), book, &name);
    let existing = lib
        .marks(id)
        .into_iter()
        .filter(|m| m.kind == MarkKind::Bookmark)
        .find_map(|m| (page_of(&doc, m.spot) == Some(here)).then_some(m.id));
    let at = Bookmarking {
        book,
        name: &name,
        spot,
        label: bookmark_label(&doc, page),
        existing,
    };
    session.study_toggle_bookmark(at, wall_ms());
}
