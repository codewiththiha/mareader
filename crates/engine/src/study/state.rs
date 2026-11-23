//! Engine-owned study state: loose marks, gloss answers, and explainer jobs.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use mareader_appearance::Rgb;
use mareader_library::{
    BookId, DeviceId, Library, Mark, MarkId, MarkKind, NewMark, Pen, Spot, Style,
};
use mareader_study::{Explainer, Failure, Unavailable, WordInfo};

use super::Action;
use super::asking::{self, Got, Said};
use super::loose::{loose_book, loose_content};
use super::words::Asked;
use crate::command::Command;
use crate::session::Waker;
use crate::toast::Toast;

/// Which answer a card shows: a mark's, or one kept by nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Mark(MarkId),
    Loose(u64),
}

/// A page to bookmark, or the bookmark already on it.
pub struct Bookmarking<'a> {
    pub book: Option<BookId>,
    pub name: &'a str,
    pub spot: Spot,
    /// Where it is, e.g. its chapter, kept with it.
    pub label: String,
    pub existing: Option<MarkId>,
}

/// The reader's highlight, as the app gathers it.
pub struct Highlighted<'a> {
    pub book: Option<BookId>,
    pub name: &'a str,
    pub text: String,
    pub spot: Spot,
    pub pen: Pen,
    /// A colour picked for it; none follows the pen's.
    pub color: Option<Rgb>,
}

/// Engine-owned study marks, answers, and active explainer lookups.
pub struct State {
    explainer: Arc<dyn Explainer>,
    /// Answers on this thread, as tests and the browser need.
    inline: bool,
    /// Documents outside the library keep marks here, for the session.
    loose: Library,
    /// Answers this session; a mark's are saved with it too.
    answers: HashMap<Key, Said>,
    /// Answers still arriving, as far as they got.
    partial: HashMap<Key, WordInfo>,
    failed: HashMap<Key, Failure>,
    asking: HashMap<Key, Receiver<Got>>,
    /// Finished answers not yet filed, as inline asks leave them.
    done: Vec<(Key, Said)>,
    next_loose: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            explainer: Arc::new(Unavailable),
            inline: false,
            loose: Library::new(DeviceId(0)),
            answers: HashMap::new(),
            partial: HashMap::new(),
            failed: HashMap::new(),
            asking: HashMap::new(),
            done: Vec::new(),
            next_loose: 0,
        }
    }
}

impl State {
    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
    }

    pub fn inline(&self) -> bool {
        self.inline
    }

    /// True while someone answers for the AI Dictionary.
    pub fn explains(&self) -> bool {
        !self.explainer.by().is_empty()
    }

    pub fn set_explainer(&mut self, explainer: Arc<dyn Explainer>) {
        // Saved answers stay; Ask again reaches the new AI.
        self.explainer = explainer;
        self.failed.clear();
    }

    /// The book's library: the app's, else the session's for `name`.
    pub fn shelf<'a>(
        &'a mut self,
        main: &'a mut Library,
        book: Option<BookId>,
        name: &str,
    ) -> (&'a mut Library, BookId) {
        if let Some(id) = book.filter(|b| main.book(*b).is_some()) {
            return (main, id);
        }
        let id = loose_book(&mut self.loose, name);
        (&mut self.loose, id)
    }

    /// The library keeping `book`'s marks, and its id there.
    pub fn lib_of<'a>(
        &'a self,
        main: &'a Library,
        book: Option<BookId>,
        name: &str,
    ) -> (&'a Library, BookId) {
        match book.filter(|b| main.book(*b).is_some()) {
            Some(id) => (main, id),
            None => (&self.loose, BookId::original(loose_content(name))),
        }
    }

    pub fn mark<'a>(&'a self, main: &'a Library, id: MarkId) -> Option<&'a Mark> {
        main.mark(id).or_else(|| self.loose.mark(id))
    }

    pub fn answer(&self, key: Key) -> Option<&Said> {
        self.answers.get(&key)
    }

    pub fn partial(&self, key: Key) -> Option<&WordInfo> {
        self.partial.get(&key)
    }

    pub fn failure(&self, key: Key) -> Option<&Failure> {
        self.failed.get(&key)
    }

    /// Allocates a loose card key for a word without a saved mark.
    pub(crate) fn next_loose_key(&mut self) -> Key {
        self.next_loose += 1;
        Key::Loose(self.next_loose)
    }

    /// Marks the explained word if it has a spot, returning its answer key.
    pub(crate) fn explain_mark(
        &mut self,
        main: &mut Library,
        book: Option<BookId>,
        name: &str,
        asked: &Asked,
        wall_ms: u64,
    ) -> Key {
        let new = asked
            .spot
            .map(|spot| NewMark::explain(asked.word.clone(), asked.context.clone(), spot));
        let (lib, book) = self.shelf(main, book, name);
        match new.and_then(|n| lib.add_mark(book, n, wall_ms)) {
            Some(id) => Key::Mark(id),
            None => {
                self.next_loose += 1;
                Key::Loose(self.next_loose)
            }
        }
    }

    /// Seeds a saved gloss or kicks off a lookup for `key`.
    pub fn prepare_card(
        &mut self,
        main: &mut Library,
        wake: Waker,
        key: Key,
        word: &str,
        context: &str,
    ) {
        // A failed answer is asked for again; a saved one is shown.
        self.failed.remove(&key);
        if let Key::Mark(id) = key
            && !self.asking.contains_key(&key)
            && !self.answers.contains_key(&key)
            && let Some(said) = self.mark(main, id).and_then(Said::of)
        {
            self.answers.insert(key, said);
        }
        if !self.answers.contains_key(&key) && !self.asking.contains_key(&key) {
            self.ask(wake, key, word, context);
        }
        self.save(main);
    }

    /// Retries a failed explanation lookup for `key`.
    pub fn retry(&mut self, main: &mut Library, wake: Waker, key: Key, word: &str, context: &str) {
        if !self.asking.contains_key(&key) {
            self.answers.remove(&key);
            self.failed.remove(&key);
            self.ask(wake, key, word, context);
        }
        self.save(main);
    }

    /// Asks the active explainer again, replacing any saved answer.
    pub fn ask_again(
        &mut self,
        main: &mut Library,
        wake: Waker,
        key: Key,
        word: &str,
        context: &str,
    ) {
        // The saved answer stays until a new one replaces it.
        if !self.asking.contains_key(&key) {
            self.answers.remove(&key);
            self.failed.remove(&key);
            self.ask(wake, key, word, context);
        }
        self.save(main);
    }

    fn ask(&mut self, wake: Waker, key: Key, word: &str, context: &str) {
        let explainer = Arc::clone(&self.explainer);
        self.partial.remove(&key);
        if self.inline || cfg!(target_arch = "wasm32") {
            match explainer.explain_by(word, context, &|_| {}) {
                Ok((info, by)) => {
                    let info = info.clamped();
                    self.done.push((key, Said { info, by }));
                }
                Err(e) => {
                    self.failed.insert(key, e);
                }
            }
            return;
        }
        let rx = asking::ask(wake, explainer, word.to_owned(), context.to_owned());
        self.asking.insert(key, rx);
    }

    /// Takes in answers, finished or still filling in; returns true on change.
    pub fn receive(&mut self, main: &mut Library) -> bool {
        let mut changed = false;
        let mut done = Vec::new();
        for (key, rx) in &self.asking {
            loop {
                match rx.try_recv() {
                    Ok(Got::Partial(info)) => {
                        self.partial.insert(*key, info.clamped());
                        changed = true;
                    }
                    Ok(Got::Done(answer)) => {
                        done.push((*key, Some(answer)));
                        changed = true;
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        done.push((*key, None));
                        changed = true;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
        }
        for (key, answer) in done {
            self.asking.remove(&key);
            self.partial.remove(&key);
            match answer {
                Some(Ok(said)) => self.done.push((key, said)),
                Some(Err(e)) => {
                    self.failed.insert(key, e);
                }
                None => {}
            }
        }
        self.save(main);
        changed
    }

    /// Files finished answers: in the session, and with their marks.
    pub fn save(&mut self, main: &mut Library) {
        let wall = crate::library::wall_ms();
        for (key, said) in std::mem::take(&mut self.done) {
            if let Key::Mark(id) = key {
                let gloss = Some(said.gloss());
                if !main.set_gloss(id, gloss.clone(), wall) {
                    self.loose.set_gloss(id, gloss, wall);
                }
            }
            self.answers.insert(key, said);
        }
    }

    /// Highlights text in `pen`; returns the mark id and whether it is new.
    pub fn highlight(
        &mut self,
        main: &mut Library,
        at: Highlighted<'_>,
        wall_ms: u64,
    ) -> Option<(MarkId, bool)> {
        let color = at.color.map(|Rgb(r, g, b)| [r, g, b]);
        let new = NewMark::highlight(at.text, at.spot, at.pen, color);
        let (lib, book) = self.shelf(main, at.book, at.name);
        let before = lib.marks(book).len();
        let id = lib.add_mark(book, new, wall_ms)?;
        Some((id, lib.marks(book).len() > before))
    }

    /// Bookmarks the page, or removes its existing bookmark with an Undo toast.
    pub(crate) fn toggle_bookmark(
        &mut self,
        main: &mut Library,
        at: Bookmarking<'_>,
        wall_ms: u64,
    ) -> Option<Toast> {
        if let Some(id) = at.existing {
            return self.remove_marks(main, vec![id], wall_ms);
        }
        let (lib, book) = self.shelf(main, at.book, at.name);
        lib.add_mark(book, NewMark::bookmark(at.label, at.spot), wall_ms);
        None
    }

    /// Removes marks, returning an Undo toast if any were alive.
    pub(crate) fn remove_marks(
        &mut self,
        main: &mut Library,
        ids: Vec<MarkId>,
        wall: u64,
    ) -> Option<Toast> {
        let bookmark = |id: &MarkId| {
            self.mark(main, *id)
                .is_some_and(|m| m.kind == MarkKind::Bookmark)
        };
        let what = if ids.iter().all(bookmark) {
            "bookmark"
        } else {
            "highlight"
        };
        let n =
            main.set_marks_alive(&ids, false, wall) + self.loose.set_marks_alive(&ids, false, wall);
        (n > 0).then(|| {
            let message = match n {
                1 => format!("Removed {what}"),
                n => format!("Removed {n} {what}s"),
            };
            Toast::info(message).undo(Command::Study(Action::Restore(ids)))
        })
    }

    /// Removes marks quietly, as a cancelled note's placeholder highlight.
    pub(crate) fn drop_marks(&mut self, main: &mut Library, ids: &[MarkId]) {
        let wall = crate::library::wall_ms();
        main.set_marks_alive(ids, false, wall);
        self.loose.set_marks_alive(ids, false, wall);
    }

    /// Brings removed marks back (Undo).
    pub(crate) fn restore_marks(&mut self, main: &mut Library, ids: &[MarkId], wall: u64) {
        main.set_marks_alive(ids, true, wall);
        self.loose.set_marks_alive(ids, true, wall);
    }

    /// Updates a single highlight's style.
    pub fn restyle(&mut self, main: &mut Library, id: MarkId, style: Style, wall: u64) {
        if !main.set_style(id, style, wall) {
            self.loose.set_style(id, style, wall);
        }
    }

    /// Restyles picked highlights; returns an Undo toast when any changed.
    pub(crate) fn restyle_all(
        &mut self,
        main: &mut Library,
        changes: Vec<(MarkId, Style)>,
        skipped: usize,
        wall: u64,
    ) -> Toast {
        let mut before = Vec::new();
        for (id, style) in changes {
            let Some(was) = self.mark(main, id).map(Mark::look) else {
                continue;
            };
            if main.set_style(id, style, wall) || self.loose.set_style(id, style, wall) {
                before.push((id, was));
            }
        }
        let n = before.len();
        let message = match (n, skipped) {
            (0, _) => "That look is kept for marks of their own".to_owned(),
            (1, 0) => "Restyled 1 highlight".to_owned(),
            (n, 0) => format!("Restyled {n} highlights"),
            (n, s) => format!("Restyled {n} of {} highlights", n + s),
        };
        match n {
            0 => Toast::info(message),
            _ => Toast::info(message).undo(Command::Study(Action::Unstyle(before))),
        }
    }

    /// Reverts picked highlights' previous styles (Undo).
    pub(crate) fn unstyle(&mut self, main: &mut Library, looks: &[(MarkId, Style)], wall: u64) {
        for &(id, style) in looks {
            if !main.set_style(id, style, wall) {
                self.loose.set_style(id, style, wall);
            }
        }
    }

    /// Saves note text on mark `id`.
    pub fn set_note(&mut self, main: &mut Library, id: MarkId, text: String, wall: u64) {
        if !main.set_note(id, Some(text.clone()), wall) {
            self.loose.set_note(id, Some(text), wall);
        }
    }

    /// Deletes `id`'s note, returning an Undo toast when one was present.
    pub(crate) fn forget_note(
        &mut self,
        main: &mut Library,
        id: MarkId,
        wall: u64,
    ) -> Option<Toast> {
        let was = self.mark(main, id).and_then(|m| m.note.value.clone());
        if !main.set_note(id, None, wall) {
            self.loose.set_note(id, None, wall);
        }
        was.map(|text| {
            let undo = Command::Study(Action::RestoreNote(id, text));
            Toast::info("Deleted note").undo(undo)
        })
    }

    /// Restores a deleted note's text (Undo).
    pub(crate) fn restore_note(&mut self, main: &mut Library, id: MarkId, text: String, wall: u64) {
        if !main.set_note(id, Some(text.clone()), wall) {
            self.loose.set_note(id, Some(text), wall);
        }
    }
}
