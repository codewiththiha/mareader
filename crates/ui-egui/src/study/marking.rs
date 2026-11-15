//! Marks on the page: the menu over one, and picking several at once.

use std::sync::Arc;

use egui::{Context, Pos2, Rect};
use mareader_engine::session::Session;
use mareader_library::{BookId, Library, Mark, MarkId, MarkKind};
use mareader_settings::HighlightSettings;
use mareader_workspace::PaneId;

use super::{
    Bookmarks, Card, Key, LISTED_CAP, Listed, MarkMenu, StudyState, StudyUi, ink, marks, picking,
};

impl StudyUi {
    /// `book`'s marks, else the session document `name`'s; AI marks last.
    /// Bookmarks hang ribbons instead.
    pub fn marks_of(
        &self,
        state: &StudyState,
        main: &Library,
        book: Option<BookId>,
        name: &str,
        look: &HighlightSettings,
    ) -> Arc<[marks::Shown]> {
        self.listed(state, main, book, name, look).0
    }

    /// `book`'s marks to draw and its bookmarks, listed again on a change.
    fn listed(
        &self,
        state: &StudyState,
        main: &Library,
        book: Option<BookId>,
        name: &str,
        look: &HighlightSettings,
    ) -> (Arc<[marks::Shown]>, Bookmarks) {
        let (lib, id) = state.lib_of(main, book, name);
        let stamp = lib.marks_stamp();
        let mut listed = self.listed.borrow_mut();
        let same = |l: &&Listed| l.stamp == stamp && l.book == id && l.look == *look;
        if let Some(l) = listed.iter().find(same) {
            return (Arc::clone(&l.shown), Arc::clone(&l.bookmarks));
        }
        let all = lib.marks(id);
        let (kept, drawn): (Vec<&Mark>, Vec<&Mark>) =
            all.into_iter().partition(|m| m.kind == MarkKind::Bookmark);
        let mut shown: Vec<marks::Shown> = drawn
            .into_iter()
            .map(|m| marks::Shown {
                id: m.id,
                spot: m.spot,
                ink: ink(m, look),
                noted: m.note.value.is_some(),
            })
            .collect();
        shown.sort_by_key(|m| m.ink.ai);
        let bookmarks: Bookmarks = kept.iter().map(|m| (m.id, m.spot)).collect();
        let shown: Arc<[marks::Shown]> = shown.into();
        listed.retain(|l| l.book != id);
        if listed.len() >= LISTED_CAP {
            listed.remove(0);
        }
        listed.push(Listed {
            stamp,
            book: id,
            look: look.clone(),
            shown: Arc::clone(&shown),
            bookmarks: Arc::clone(&bookmarks),
        });
        (shown, bookmarks)
    }

    /// A click on a mark: its card again; a highlight's note or menu.
    pub fn open_mark(
        &mut self,
        ctx: &Context,
        session: &mut Session,
        pane: PaneId,
        id: MarkId,
        at: Rect,
    ) {
        let info = session.study().mark(session.library_model(), id).map(|m| {
            let noted = m.note.value.is_some();
            (m.kind, noted, m.word.clone(), m.context.clone())
        });
        let Some((kind, has_note, word, context)) = info else {
            return;
        };
        let (state, model) = (session.study(), session.library_model());
        if kind == MarkKind::Highlight && has_note {
            self.open_note(state, model, pane, id, at, false);
            return;
        }
        if kind == MarkKind::Highlight {
            let p = ctx.pointer_latest_pos().unwrap_or(at.left_bottom());
            self.menu_at(state, model, pane, id, p, at);
            return;
        }
        self.open(session, pane, Key::Mark(id), (word, context), at);
    }

    /// The right-click menu on a mark.
    pub fn open_menu(
        &mut self,
        state: &StudyState,
        main: &Library,
        pane: PaneId,
        mark: MarkId,
        at: Pos2,
        words: Rect,
    ) {
        self.menu_at(state, main, pane, mark, at, words);
    }

    fn menu_at(
        &mut self,
        state: &StudyState,
        main: &Library,
        pane: PaneId,
        mark: MarkId,
        at: Pos2,
        rect: Rect,
    ) {
        let highlight = state
            .mark(main, mark)
            .filter(|m| m.kind == MarkKind::Highlight);
        let noted = highlight.map(|m| m.note.value.is_some());
        let style = highlight.map(Mark::look);
        self.card = None;
        self.note = None;
        self.menu = Some(MarkMenu {
            pane,
            mark,
            at,
            rect,
            noted,
            style,
            seen: true,
        });
    }

    /// Where the mark menu was drawn, for tests.
    pub fn menu_rect(&self) -> Option<Rect> {
        self.menu.and(self.menu_rect)
    }

    /// Picking several marks begins with `mark`, or takes it in.
    pub fn pick(&mut self, pane: PaneId, mark: MarkId) {
        self.card = None;
        self.menu = None;
        self.note = None;
        match &mut self.picking {
            Some(p) if p.pane == pane => p.add(&[mark]),
            _ => self.picking = Some(picking::Picking::new(pane, mark)),
        }
    }

    pub fn toggle(&mut self, pane: PaneId, mark: MarkId) {
        if let Some(p) = self.picking.as_mut().filter(|p| p.pane == pane) {
            p.toggle(mark);
        }
    }

    pub fn seen(&mut self, pane: PaneId, page: Vec<MarkId>, all: Vec<MarkId>) {
        if let Some(p) = self.picking.as_mut().filter(|p| p.pane == pane) {
            (p.page, p.all) = (page, all);
        }
    }

    /// The marks picked in `pane`, while picking there.
    pub fn picked(&self, pane: PaneId) -> Option<Vec<MarkId>> {
        let p = self.picking.as_ref().filter(|p| p.pane == pane)?;
        Some(p.ids.clone())
    }

    pub fn picking(&self) -> bool {
        self.picking.is_some()
    }

    /// The open card's mark, seen this frame at `at`, or maybe there.
    pub fn anchor(&mut self, pane: PaneId, at: Option<Rect>) {
        if let Some(card) = self.card.as_mut().filter(|c| c.pane == pane) {
            card.at = at.unwrap_or(card.at);
            card.seen = true;
        }
        if let Some(note) = self.note.as_mut().filter(|n| n.pane == pane) {
            note.at = at.unwrap_or(note.at);
            note.seen = true;
        }
        // The menu rides with its words, as the card does.
        if let Some(menu) = self.menu.as_mut().filter(|m| m.pane == pane) {
            if let Some(at) = at {
                menu.at += at.min - menu.rect.min;
                menu.rect = at;
            }
            menu.seen = true;
        }
    }

    /// Closes any card, menu, or picking bar affected by removed marks.
    pub(super) fn act_remove(&mut self, ids: Vec<MarkId>) {
        let gone = |c: &Card| matches!(c.key, Key::Mark(id) if ids.contains(&id));
        if self.card.as_ref().is_some_and(gone) {
            self.card = None;
        }
        self.menu = None;
        self.picking = None;
    }
}
