//! Engine-owned study state: loose marks, gloss answers, and explainer jobs.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::TryRecvError;

use mareader_appearance::Rgb;
use mareader_library::{BookId, DeviceId, Library, Mark, MarkId, MarkKind, NewMark};
use mareader_study::{Explainer, Failure, Pos, Unavailable, WordInfo};
use mareader_workspace::PaneId;

use super::Action;
use super::asking::{self, Ask, Got, Said};
use super::loose::{loose_book, loose_content};
use super::words::Asked;
use crate::ai_dict::{Filing, Offer};
use crate::command::Command;
use crate::session::Waker;
use crate::toast::Toast;

#[path = "state_looks.rs"]
mod looks;

pub use super::marking::{Bookmarking, Highlighted};

/// Which answer a card shows: a mark's, or one kept by nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    Mark(MarkId),
    Loose(u64),
}

/// Who asked a word whose answer still comes, for the AI stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asker {
    pub pane: PaneId,
    pub word: String,
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
    /// Dropping one stops its explainer.
    asking: HashMap<Key, Ask>,
    /// Failed since the last save: their unanswered marks go.
    lapsed: Vec<Key>,
    askers: HashMap<Key, Asker>,
    /// The tagger's class per card, a guess the AI is given.
    hints: HashMap<Key, Pos>,
    /// Finished answers not yet filed, as inline asks leave them.
    done: Vec<(Key, Said, Option<&'static str>)>,
    /// Answers filed with their marks, still to go to the AI dictionary.
    filed: Vec<(Key, Said, Option<&'static str>)>,
    /// Answers sent to the AI dictionary, and the replace offers raised.
    pub(crate) ai_dict: Filing,
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
            lapsed: Vec::new(),
            askers: HashMap::new(),
            hints: HashMap::new(),
            done: Vec::new(),
            filed: Vec::new(),
            ai_dict: Filing::default(),
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

    /// The language the explainer writes in for the AI dictionary, if any.
    pub fn language(&self) -> Option<&'static str> {
        self.explainer.language()
    }

    /// The kept meaning `key`'s answer differs from, the reader may swap.
    pub fn offer(&self, key: Key) -> Option<&Offer> {
        self.ai_dict.offer(key)
    }

    /// Answers that came since last taken, for the AI dictionary.
    pub(crate) fn take_filed(&mut self) -> Vec<(Key, Said, Option<&'static str>)> {
        std::mem::take(&mut self.filed)
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

    /// The marks whose answers are still coming.
    pub fn asking_marks(&self) -> Vec<MarkId> {
        let marks = self.asking.keys().filter_map(|k| match k {
            Key::Mark(id) => Some(*id),
            Key::Loose(_) => None,
        });
        marks.collect()
    }

    pub fn failure(&self, key: Key) -> Option<&Failure> {
        self.failed.get(&key)
    }

    /// Notes `pane` asked `word` for `key`, while its answer comes.
    pub fn asked_by(&mut self, key: Key, pane: PaneId, word: &str) {
        if self.asking.contains_key(&key) {
            let word = word.to_owned();
            self.askers.insert(key, Asker { pane, word });
        }
    }

    /// The words whose answers still come, and who asked them.
    pub fn askers(&self) -> impl Iterator<Item = (Key, &Asker)> {
        let asked = |key: &Key| self.askers.get(key).map(|asker| (*key, asker));
        self.asking.keys().filter_map(asked)
    }

    /// Stops `key`'s ask, its explainer too; says whether one was coming.
    pub fn drop_ask(&mut self, key: Key) -> bool {
        self.partial.remove(&key);
        self.askers.remove(&key);
        self.asking.remove(&key).is_some()
    }

    /// The reader cancels `key`'s ask: it stops, and a mark never
    /// answered goes, its word as it was; says whether one was coming.
    pub(crate) fn cancel_ask(&mut self, main: &mut Library, key: Key) -> bool {
        let stopped = self.drop_ask(key);
        if stopped {
            self.failed.remove(&key);
            self.unmark(main, key);
        }
        stopped
    }

    /// Drops `key`'s mark quietly if it never got an answer.
    fn unmark(&mut self, main: &mut Library, key: Key) {
        let Key::Mark(id) = key else {
            return;
        };
        let bare = |m: &Mark| m.kind == MarkKind::Explain && m.gloss.value.is_none();
        if self.mark(main, id).is_some_and(bare) {
            self.drop_marks(main, &[id]);
        }
    }

    /// The tagger's class for `key`'s word, for the next ask.
    pub fn hint(&mut self, key: Key, pos: Pos) {
        self.hints.insert(key, pos);
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

    /// Retries a failed explanation lookup for `key`, its dropped mark
    /// back while it asks.
    pub fn retry(&mut self, main: &mut Library, wake: Waker, key: Key, word: &str, context: &str) {
        if let Key::Mark(id) = key {
            self.restore_marks(main, &[id], crate::library::wall_ms());
        }
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
        let explainer = self
            .explainer
            .snapshot()
            .unwrap_or_else(|| Arc::clone(&self.explainer));
        let language = explainer.language();
        let hint = self.hints.get(&key).copied();
        self.partial.remove(&key);
        if (self.inline && !explainer.waits()) || cfg!(target_arch = "wasm32") {
            match explainer.explain_by((word, context, hint), &|_| {}) {
                Ok((info, by)) => {
                    let info = info.clamped();
                    let word = word.to_owned();
                    self.done.push((key, Said { info, by, word }, language));
                }
                Err(e) => {
                    self.failed.insert(key, e);
                    self.lapsed.push(key);
                }
            }
            return;
        }
        let words = (word.to_owned(), context.to_owned(), hint);
        let rx = asking::ask(wake, explainer, words);
        self.asking.insert(key, rx);
    }

    /// Takes in answers, finished or still filling in; returns true on change.
    pub fn receive(&mut self, main: &mut Library) -> bool {
        let mut changed = false;
        let mut done = Vec::new();
        for (key, ask) in &self.asking {
            loop {
                match ask.rx.try_recv() {
                    Ok(Got::Partial(info)) => {
                        self.partial.insert(*key, info.clamped());
                        changed = true;
                    }
                    Ok(Got::Done(answer)) => {
                        done.push((*key, Some(answer), ask.language));
                        changed = true;
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        done.push((*key, None, ask.language));
                        changed = true;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
        }
        for (key, answer, language) in done {
            self.asking.remove(&key);
            self.askers.remove(&key);
            self.partial.remove(&key);
            match answer {
                Some(Ok(said)) => self.done.push((key, said, language)),
                Some(Err(e)) => {
                    self.failed.insert(key, e);
                    self.lapsed.push(key);
                }
                None => {}
            }
        }
        self.save(main);
        changed
    }

    /// Files finished answers: in the session, and with their marks. A
    /// failed ask's mark that was never answered goes.
    pub fn save(&mut self, main: &mut Library) {
        for key in std::mem::take(&mut self.lapsed) {
            self.unmark(main, key);
        }
        let wall = crate::library::wall_ms();
        for (key, said, language) in std::mem::take(&mut self.done) {
            if let Key::Mark(id) = key {
                let gloss = Some(said.gloss());
                if !main.set_gloss(id, gloss.clone(), wall) {
                    self.loose.set_gloss(id, gloss, wall);
                }
            }
            self.filed.push((key, said.clone(), language));
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
        // Nothing asks for a mark that went.
        for &id in &ids {
            self.drop_ask(Key::Mark(id));
        }
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
        for &id in ids {
            self.drop_ask(Key::Mark(id));
        }
        let wall = crate::library::wall_ms();
        main.set_marks_alive(ids, false, wall);
        self.loose.set_marks_alive(ids, false, wall);
    }

    /// Brings removed marks back (Undo).
    pub(crate) fn restore_marks(&mut self, main: &mut Library, ids: &[MarkId], wall: u64) {
        main.set_marks_alive(ids, true, wall);
        self.loose.set_marks_alive(ids, true, wall);
    }
}
