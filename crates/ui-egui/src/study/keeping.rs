//! What a reader keeps on a book: its notes, bookmarks and highlights.

use std::sync::Arc;

use egui::Rect;
use mareader_engine::command::Command;
use mareader_engine::session::Session;
use mareader_engine::study::Action;
use mareader_library::{BookId, Library, MarkId, MarkKind};
use mareader_settings::HighlightSettings;
use mareader_workspace::PaneId;

use super::{Bookmarks, Note, StudyUi, ink};
use mareader_engine::study::state::State as StudyState;

impl StudyUi {
    /// `book`'s bookmarks, each with where it is.
    pub fn bookmarks_of(
        &self,
        state: &StudyState,
        main: &Library,
        book: Option<BookId>,
        name: &str,
    ) -> Bookmarks {
        let (lib, id) = state.lib_of(main, book, name);
        let stamp = lib.marks_stamp();
        let listed = self.listed.borrow();
        // Drawing a pane lists its marks first; the bookmarks come along.
        if let Some(l) = listed.iter().find(|l| l.stamp == stamp && l.book == id) {
            return Arc::clone(&l.bookmarks);
        }
        let marks = lib.marks(id).into_iter();
        let kept = marks.filter(|m| m.kind == MarkKind::Bookmark);
        kept.map(|m| (m.id, m.spot)).collect()
    }

    /// The book's highlights and bookmarks for the panel, in reading order.
    pub fn notes_of(
        &self,
        state: &StudyState,
        main: &Library,
        book: Option<BookId>,
        name: &str,
        look: &HighlightSettings,
    ) -> Vec<crate::notes_panel::Row> {
        let (lib, id) = state.lib_of(main, book, name);
        let mut out: Vec<crate::notes_panel::Row> = lib
            .marks(id)
            .into_iter()
            .filter(|m| m.kind != MarkKind::Explain)
            .map(|m| crate::notes_panel::Row {
                id: m.id,
                spot: m.spot,
                quote: m.word.clone(),
                note: m.note.value.clone(),
                ink: ink(m, look).color,
                pen: m.look().pen,
                added: m.added,
                bookmark: m.kind == MarkKind::Bookmark,
            })
            .collect();
        out.sort_by_key(|r| crate::notes_panel::order(&r.spot));
        out
    }

    /// Opens a draft note on a highlight already created by the session.
    pub fn open_created_note(
        &mut self,
        state: &StudyState,
        main: &Library,
        pane: PaneId,
        id: MarkId,
        made: bool,
        rect: Rect,
    ) {
        self.open_note(state, main, pane, id, rect, true);
        if let Some(n) = &mut self.note {
            n.placeholder = made;
        }
    }

    /// Opens `id`'s note beside `at`: a draft if `write`, else to read.
    pub(super) fn open_note(
        &mut self,
        state: &StudyState,
        main: &Library,
        pane: PaneId,
        id: MarkId,
        at: Rect,
        write: bool,
    ) {
        let Some(mark) = state.mark(main, id).cloned() else {
            return;
        };
        self.card = None;
        self.menu = None;
        self.note_rect = None;
        let draft = write.then(|| mark.note.value.clone().unwrap_or_default());
        self.note = Some(Note {
            pane,
            mark,
            at,
            seen: true,
            draft,
            placeholder: false,
            focus: 2,
        });
    }

    /// Keeps the open draft; blank deletes the note or its new highlight.
    pub(super) fn save_note(&mut self, session: &mut Session, id: MarkId) {
        let Some(n) = self.note.take_if(|n| n.mark.id == id) else {
            return;
        };
        let Some(draft) = n.draft else {
            return;
        };
        let text = draft.trim();
        if text.is_empty() {
            if n.placeholder {
                session.execute(Command::Study(Action::DropMarks(vec![id])), 0.0);
            } else if n.mark.note.value.is_some() {
                session.execute(Command::Study(Action::DeleteNote(id)), 0.0);
            }
            return;
        }
        let save = Command::Study(Action::SetNote(id, text.to_owned()));
        session.execute(save, 0.0);
    }

    /// Closes the note unsaved; a highlight made for it goes too.
    pub(super) fn cancel_note(&mut self, session: &mut Session) {
        if let Some(n) = self.note.take().filter(|n| n.placeholder) {
            session.execute(Command::Study(Action::DropMarks(vec![n.mark.id])), 0.0);
        }
    }

    /// The open note's mark, and its draft while written; for tests.
    pub fn note_open(&self) -> Option<(MarkId, Option<&str>)> {
        let n = self.note.as_ref()?;
        Some((n.mark.id, n.draft.as_deref()))
    }
}
