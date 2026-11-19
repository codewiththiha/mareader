//! Study marks, notes, bookmarks, and explainer lookups inside [`Session`].

use std::sync::Arc;

use mareader_appearance::Rgb;
use mareader_library::{
    BookId, LearnedBy, MarkId, MarkKind, NewStudyWord, Spot, StudyProgress, StudyWordId, Style,
    WordStatus,
};
use mareader_study::{Avail, Explainer, StudyEnv, StudyMode};
use mareader_workspace::PaneId;

use crate::event::Event;
use crate::export::{Entry, Format, Sheet, file_name, render};
use crate::notes_panel::{bookmark_chapter, page_of};
use crate::study::state::{Bookmarking, Highlighted, Key, State};
use crate::study::words::Asked;
use crate::toast::Toast;

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

    /// A pane's study mode, off until set.
    pub fn study_mode(&self, pane: PaneId) -> StudyMode {
        self.study.mode(pane)
    }

    /// Both study modes' availability on this machine.
    pub fn study_availability(&self) -> (Avail, Avail) {
        let env = StudyEnv {
            cefr: self.vocab.ready(),
            ai: self.study.explains(),
            dict: !self.dict().ready().is_empty(),
            english: true,
        };
        mareader_study::availability(env)
    }

    /// Turns study mode on or off in `pane`, keeping progress.
    pub fn study_set_mode(&mut self, pane: PaneId, mode: StudyMode) {
        let (proactive, reactive) = self.study_availability();
        let blocked = match mode {
            StudyMode::Proactive => (!proactive.ok).then_some(proactive),
            StudyMode::Reactive => (!reactive.ok).then_some(reactive),
            StudyMode::Off => None,
        };
        if let Some(avail) = blocked {
            let why = avail.reason.unwrap_or("Study mode can't run here.");
            self.emit(Event::Toast(Toast::warning(why.to_owned())));
            self.study.set_mode(pane, StudyMode::Off);
            return;
        }
        self.study.set_mode(pane, mode);
        self.study_record_progress(pane, None, mode);
    }

    /// Records a deck or recall word in `pane`'s book.
    pub fn study_record_word(
        &mut self,
        pane: PaneId,
        record: crate::study::WordRecord,
    ) -> Option<StudyWordId> {
        let book = self.book_in(pane);
        let name = self.documents().title(pane).unwrap_or_default().to_owned();
        let wall = crate::library::wall_ms();
        let (lib, id) = self.study.shelf(&mut self.library.model, book, &name);
        lib.add_study_word(
            id,
            NewStudyWord {
                lemma: record.lemma,
                class: record.class,
                band: record.band,
                meaning: record.meaning,
                context: record.context,
                spot: record.spot,
                by: record.by,
                status: record.status,
                attempts: record.attempts,
                first_try: record.first_try,
            },
            wall,
        )
    }

    /// Records a word as memorized in `pane`'s book.
    pub fn study_mark_known(&mut self, pane: PaneId, lemma: &str, class: &str) {
        let page = self.documents().current(pane).unwrap_or(0);
        let record = crate::study::WordRecord {
            lemma: lemma.to_owned(),
            class: class.to_owned(),
            band: 0,
            meaning: String::new(),
            context: String::new(),
            spot: Spot::Page {
                page,
                rect: [0.0; 4],
            },
            by: LearnedBy::Card,
            status: WordStatus::Memorized,
            attempts: 0,
            first_try: false,
        };
        self.study_record_word(pane, record);
    }

    /// Sets a recorded word's status, as Space does.
    pub fn study_set_word_status(&mut self, id: StudyWordId, status: WordStatus) {
        let wall = crate::library::wall_ms();
        if !self.library.model.set_study_status(id, status, wall) {
            self.study.loose_status(id, status, wall);
        }
    }

    /// Forgets recorded words, as Space does.
    pub fn study_forget_words(&mut self, ids: &[StudyWordId]) {
        let wall = crate::library::wall_ms();
        self.library.model.set_study_words_alive(ids, false, wall);
        self.study.loose_forget(ids, wall);
    }

    /// Every recorded study word, across the library and loose books.
    pub fn study_words(&self) -> Vec<&mareader_library::StudyWord> {
        let mut out = self.library.model.study_words(None);
        out.extend(self.study.loose_words());
        out
    }

    /// The `lemma|class` keys memorized in any book.
    pub fn study_memorized_keys(&self) -> Vec<String> {
        let mut out = self.library.model.memorized_keys();
        out.extend(self.study.loose_memorized_keys());
        out.sort();
        out.dedup();
        out
    }

    /// Unlocks `pane`'s book up to `spot` and keeps the mode there.
    pub fn study_unlock(&mut self, pane: PaneId, spot: Spot) {
        self.study_record_progress(pane, Some(spot), self.study.mode(pane));
    }

    fn study_record_progress(&mut self, pane: PaneId, unlocked_to: Option<Spot>, mode: StudyMode) {
        let Some(book) = self.book_in(pane) else {
            return;
        };
        let wall = crate::library::wall_ms();
        let kept = self.library.model.study_progress(book).cloned();
        let progress = StudyProgress {
            unlocked_to: unlocked_to.or_else(|| kept.as_ref().and_then(|p| p.unlocked_to)),
            mode: match mode {
                StudyMode::Off => "off".to_owned(),
                StudyMode::Proactive => "proactive".to_owned(),
                StudyMode::Reactive => "reactive".to_owned(),
            },
        };
        self.library
            .model
            .record_study_progress(book, progress, wall);
    }

    /// Whether proactive mode in `pane` locks `page` still.
    pub fn study_blocks_page(&self, pane: PaneId, page: usize) -> bool {
        if self.study.mode(pane) != StudyMode::Proactive {
            return false;
        }
        let Some(book) = self.book_in(pane) else {
            return false;
        };
        match self.library.model.study_progress(book) {
            Some(progress) => match progress.unlocked_to {
                Some(Spot::Page { page: up, .. }) => page > up,
                _ => page > 0,
            },
            None => page > 0,
        }
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
}
