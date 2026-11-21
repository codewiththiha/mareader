//! Study marks, notes, bookmarks, and explainer lookups inside [`Session`].

use std::sync::Arc;

use mareader_appearance::Rgb;
use mareader_library::{BookId, MarkId, MarkKind, Style};
use mareader_settings::Settings;
use mareader_study::{CardMaker, Explainer, StudyJudge, StudyMode};
use mareader_workspace::PaneId;

use crate::event::Event;
use crate::export::{Entry, Format, Sheet, file_name, render};
use crate::notes_panel::{bookmark_chapter, page_of};
use crate::study::state::{Bookmarking, Highlighted, Key, State};
use crate::study::words::Asked;

use super::Session;

impl Session {
    /// Engine-owned study state, read-only.
    pub fn study(&self) -> &State {
        &self.study
    }

    /// Replaces the AI Dictionary explainer used for new word lookups.
    pub fn study_set_explainer(&mut self, explainer: Arc<dyn Explainer>) {
        self.study.set_explainer(explainer);
    }

    /// Polls active explainer threads and files completed glosses into marks.
    pub(crate) fn study_receive(&mut self) -> bool {
        self.study.receive(&mut self.library.model)
    }

    /// Marks an explained word and seeds or starts its lookup.
    pub fn study_explain(
        &mut self,
        book: Option<BookId>,
        name: &str,
        asked: &Asked,
        wall_ms: u64,
    ) -> Key {
        let key = self
            .study
            .explain_mark(&mut self.library.model, book, name, asked, wall_ms);
        let wake = Arc::clone(&self.waker);
        self.study.prepare_card(
            &mut self.library.model,
            wake,
            key,
            &asked.word,
            &asked.context,
        );
        key
    }

    /// Seeds a saved gloss or kicks off a lookup for an opened mark card.
    pub fn study_prepare_card(&mut self, key: Key, word: &str, context: &str) {
        let wake = Arc::clone(&self.waker);
        self.study
            .prepare_card(&mut self.library.model, wake, key, word, context);
    }

    /// Starts an unanchored AI explanation for an open word card.
    pub fn study_explain_loose(&mut self, word: &str, context: &str) -> Key {
        let key = self.study.next_loose_key();
        self.study_prepare_card(key, word, context);
        key
    }

    /// Retries a failed explanation lookup.
    pub fn study_retry(&mut self, key: Key, word: &str, context: &str) {
        let wake = Arc::clone(&self.waker);
        self.study
            .retry(&mut self.library.model, wake, key, word, context);
    }

    /// Asks the active explainer again, replacing any saved answer.
    pub fn study_ask_again(&mut self, key: Key, word: &str, context: &str) {
        let wake = Arc::clone(&self.waker);
        self.study
            .ask_again(&mut self.library.model, wake, key, word, context);
    }

    /// Highlights text in `pen`; returns the mark id and whether it is new.
    pub(crate) fn study_highlight(
        &mut self,
        at: Highlighted<'_>,
        wall_ms: u64,
    ) -> Option<(MarkId, bool)> {
        self.study.highlight(&mut self.library.model, at, wall_ms)
    }

    /// Bookmarks a page, or removes its existing bookmark with an Undo toast.
    pub(crate) fn study_toggle_bookmark(&mut self, at: Bookmarking<'_>, wall_ms: u64) {
        if let Some(toast) = self
            .study
            .toggle_bookmark(&mut self.library.model, at, wall_ms)
        {
            self.emit(Event::Toast(toast));
        }
    }

    /// Removes marks and emits an Undo toast when any were alive.
    pub(crate) fn study_remove_marks(&mut self, ids: Vec<MarkId>) {
        let wall = crate::library::wall_ms();
        if let Some(toast) = self.study.remove_marks(&mut self.library.model, ids, wall) {
            self.emit(Event::Toast(toast));
        }
    }

    /// Removes marks quietly, as a cancelled note's placeholder highlight.
    pub(crate) fn study_drop_marks(&mut self, ids: &[MarkId]) {
        self.study.drop_marks(&mut self.library.model, ids);
    }

    /// Brings removed marks back (Undo).
    pub(crate) fn study_restore_marks(&mut self, ids: &[MarkId]) {
        let wall = crate::library::wall_ms();
        self.study.restore_marks(&mut self.library.model, ids, wall);
    }

    /// Updates a single highlight's style.
    pub(crate) fn study_restyle(&mut self, id: MarkId, style: Style) {
        let wall = crate::library::wall_ms();
        self.study.restyle(&mut self.library.model, id, style, wall);
    }

    /// Restyles picked highlights and emits an Undo toast when any changed.
    pub(crate) fn study_restyle_all(&mut self, changes: Vec<(MarkId, Style)>, skipped: usize) {
        let wall = crate::library::wall_ms();
        let toast = self
            .study
            .restyle_all(&mut self.library.model, changes, skipped, wall);
        self.emit(Event::Toast(toast));
    }

    /// Reverts picked highlights' previous styles (Undo).
    pub(crate) fn study_unstyle(&mut self, looks: &[(MarkId, Style)]) {
        let wall = crate::library::wall_ms();
        self.study.unstyle(&mut self.library.model, looks, wall);
    }

    /// Saves note text on mark `id`.
    pub(crate) fn study_set_note(&mut self, id: MarkId, text: String) {
        let wall = crate::library::wall_ms();
        self.study.set_note(&mut self.library.model, id, text, wall);
    }

    /// Deletes `id`'s note and emits an Undo toast when one was present.
    pub(crate) fn study_forget_note(&mut self, id: MarkId) {
        let wall = crate::library::wall_ms();
        if let Some(toast) = self.study.forget_note(&mut self.library.model, id, wall) {
            self.emit(Event::Toast(toast));
        }
    }

    /// Restores a deleted note's text (Undo).
    pub(crate) fn study_restore_note(&mut self, id: MarkId, text: String) {
        let wall = crate::library::wall_ms();
        self.study
            .restore_note(&mut self.library.model, id, text, wall);
    }

    /// Most recently added highlight mark on `pane`'s open book, if any.
    pub(crate) fn latest_mark(&self, pane: PaneId) -> Option<MarkId> {
        let book = self.book_in(pane);
        let name = self.documents.title(pane).unwrap_or_default();
        let (lib, id) = self.study.lib_of(&self.library.model, book, name);
        lib.marks(id)
            .into_iter()
            .rfind(|m| m.kind == MarkKind::Highlight)
            .map(|m| m.id)
    }

    /// Renders `pane`'s highlights and notes as `format`, with a file name.
    pub(crate) fn export_notes(&self, pane: PaneId, format: Format) -> (String, String) {
        let book = self.book_in(pane);
        let title = self.documents.title(pane).unwrap_or("notes");
        let place = self.documents.place(pane);
        let doc = place.and_then(|p| self.documents.document(p.doc));
        let (lib, id) = self.study.lib_of(&self.library.model, book, title);
        let look = &self.settings.study.highlights;
        let entries: Vec<Entry> = lib
            .marks(id)
            .into_iter()
            .filter(|m| m.kind == MarkKind::Highlight)
            .map(|m| {
                let page = doc.and_then(|d| page_of(d, m.spot));
                let chapter = doc.and_then(|d| page.and_then(|p| bookmark_chapter(d, p)));
                let style = m.look();
                let color = style
                    .color
                    .map(|[r, g, b]| Rgb(r, g, b))
                    .filter(|c| look.allowed(style.pen, *c))
                    .unwrap_or_else(|| look.color(style.pen));
                Entry {
                    quote: m.word.clone(),
                    note: m.note.value.clone(),
                    page,
                    chapter,
                    pen: style.pen,
                    color,
                    added: m.added,
                }
            })
            .collect();
        let sheet = Sheet {
            title,
            entries: &entries,
            now_ms: crate::library::wall_ms(),
        };
        (file_name(title, format), render(&sheet, format))
    }

    /// Study mode: on or off, where each book's lock stands, which words are
    /// known, and any decks being prepared ahead.
    pub fn study_mode(&self) -> &crate::study::mode::Mode {
        &self.study_mode
    }

    #[allow(dead_code)]
    pub(crate) fn study_mode_mut(&mut self) -> &mut crate::study::mode::Mode {
        &mut self.study_mode
    }

    pub(crate) fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Configures who writes a batch's cards (the AI, a dictionary pack, or
    /// nobody), as with the gloss explainer.
    pub fn study_set_card_maker(&mut self, card_maker: Arc<dyn CardMaker>) {
        self.study_mode.set_card_maker(card_maker);
    }

    /// Configures who judges a recall attempt (the model, or nobody).
    pub fn study_set_judge(&mut self, judge: Arc<dyn StudyJudge>) {
        self.study_mode.set_judge(judge);
    }

    /// Turns study mode on or off, persisting the choice with the settings.
    pub fn study_mode_set(&mut self, mode: StudyMode) {
        self.settings_mut().study.study_mode.mode = mode;
        self.study_mode.set_mode(mode);
    }

    /// Marks a studied word memorized, globally across books.
    pub fn study_mark_known(&mut self, key: &str) {
        let band = self.study_mode.words.get(key).map_or(0, |state| state.band);
        self.study_mode.mark_known(key, band);
    }

    /// Puts a memorized word back into rotation.
    pub fn study_unmemorize(&mut self, key: &str) {
        self.study_mode.unmemorize(key);
    }
}
