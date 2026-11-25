//! Engine-owned services held by [`Session`].

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::Arc;

use mareader_cefr::Role;
use mareader_dict::{Query, Reply, Ticket};
use mareader_session::{DocId, Document, TextLayer};
use mareader_settings::{PronunciationSettings, WordList};

use crate::dict::{Action as DictAction, Download, Service as DictService};
use crate::event::Event;
use crate::search::Search;
use crate::sounds::{Line, Sounds, Spoken};
use crate::toast::Toast;
use crate::triggers::ClipAction;
use crate::vocab::{ShadingSource, Vocab, Words};
use crate::word_list::{Ask, Gather, Levels, Made, Progress};

use super::Session;

impl Session {
    pub fn vocab(&self) -> &Vocab {
        &self.vocab
    }

    pub fn dict(&self) -> &DictService {
        &self.dict
    }

    pub fn sounds(&self) -> &Sounds {
        &self.sounds
    }

    pub fn search(&self) -> &Search {
        &self.search
    }

    pub fn making_word_list(&self) -> bool {
        self.word_lists.busy()
    }

    /// Whether background service work runs inline on this thread.
    pub fn inline(&self) -> bool {
        self.inline
    }

    /// Runs background service work inline on this thread (for tests/shots).
    pub fn set_inline(&mut self, on: bool) {
        self.inline = on;
        self.save.set_inline(on);
        self.vocab.set_inline(on);
        self.sounds.set_inline(on);
        self.search.set_inline(on);
        self.library.set_inline(on);
        self.page_ai.inline = on;
        self.study.set_inline(on);
        self.dict.set_inline(on);
    }

    /// Attaches the dictionary packs directory using the session's waker.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wire_dict(&mut self, dir: PathBuf) -> Result<(), String> {
        let waker = Arc::clone(&self.waker);
        self.dict.attach(dir, waker)?;
        self.dict.set_inline(self.inline);
        Ok(())
    }

    /// Submits a dictionary lookup query, returning its ticket.
    pub fn dict_ask(&mut self, query: Query) -> Ticket {
        self.dict.ask(query)
    }

    /// Looks `word` up for a card, in the Dictionary's chosen language.
    pub fn dict_look_up(&mut self, word: &str) -> Ticket {
        let to = self.settings.dictionary.default_lang.clone();
        let word = word.to_owned();
        self.dict.ask(Query::LookUp { word, to })
    }

    /// Polls dictionary packs and queries: `(moved, changed, replies)`.
    pub(crate) fn dict_poll(&mut self) -> (bool, bool, Vec<Reply>) {
        self.dict.poll()
    }

    pub(crate) fn dict_act_pack(&mut self, action: &DictAction) -> bool {
        self.dict.act_pack(action)
    }

    /// Attaches the vocabulary dataset directory using the session's waker.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wire_vocab(&mut self, dir: PathBuf) {
        let waker = Arc::clone(&self.waker);
        if let Err(why) = self.vocab.attach(dir, waker) {
            self.emit(Event::Toast(Toast::warning(format!(
                "Word levels are off: {why}"
            ))));
            return;
        }
        self.vocab.set_inline(self.inline);
    }

    /// Asks the tagger for `word`'s role in `sentence`; with no tagger,
    /// asks its dictionary rows for its class at once.
    ///
    /// The dictionary is otherwise asked only when the tagger's answer
    /// names no class (`tick_vocab_and_audio`).
    pub fn ask_role(&mut self, word: &str, sentence: &str) {
        if !self.vocab.ask_role(word, sentence) {
            self.dict.ask_card_class(word);
        }
    }

    /// Polls vocabulary answers; whether pages changed, and the open
    /// card's word with the tagger's answer once it came.
    pub(crate) fn poll_vocab(&mut self) -> (bool, Option<(String, Option<Role>)>) {
        self.vocab.poll()
    }

    /// Polls pronunciation downloads and lookups; true if anything changed.
    pub(crate) fn poll_sounds(&mut self, now: f64) -> bool {
        self.sounds.poll(now)
    }

    /// Looks up formatted sound lines for `word` under `shown` at `now`.
    pub fn sound_lines(
        &mut self,
        word: &str,
        shown: (&PronunciationSettings, Spoken),
        now: f64,
    ) -> Option<Vec<Line>> {
        self.sounds.lines(word, shown, now)
    }

    /// Starts or updates an index search for `query` on `document`.
    pub(crate) fn ask_search(&mut self, doc: DocId, document: &Arc<Document>, query: &str) {
        let wake = Arc::clone(&self.waker);
        self.search.ask(wake, doc, document, query);
    }

    /// Polls running document searches; true if any hits arrived.
    pub(crate) fn poll_search(&mut self) -> bool {
        self.search.poll()
    }

    /// Drops searches for closed documents.
    pub(crate) fn forget_closed_searches(&mut self) {
        let documents = &self.documents;
        self.search.forget_closed(|doc| documents.document(doc));
    }

    /// Starts copying `pages` of `document` to the clipboard.
    pub(crate) fn start_page_copy(&mut self, document: Arc<Document>, pages: Vec<usize>) {
        self.page_copy.start(document, pages);
    }

    /// Completes any finished page-copy job via the host clipboard and toast.
    pub(crate) fn poll_page_copy(&mut self) {
        if let Some(read) = self.page_copy.poll() {
            self.host.set_clipboard(read.text);
            self.emit(Event::Toast(read.toast));
        }
    }

    /// Runs a trigger clip, pausing read-aloud when playback starts.
    pub fn act_clip(&mut self, doc: &Arc<Document>, clip: ClipAction) -> bool {
        let started = self.clips.act(&self.host, doc, clip);
        if started {
            self.read_aloud.pause();
        }
        started
    }

    pub(crate) fn tick_clips(&mut self) {
        self.clips.tick();
    }

    /// Starts building an instant word list using the session's waker.
    pub(crate) fn start_word_list(&mut self, ask: Ask) {
        let wake = Arc::clone(&self.waker);
        self.word_lists.start(wake, ask);
    }

    /// Advances the active instant word list using session services.
    pub(crate) fn poll_word_list(&mut self) -> Option<Result<(Made, WordList), Toast>> {
        self.word_lists.poll(&mut self.vocab, &mut self.dict)
    }

    /// Advances an open word-list preview using session services.
    pub fn step_word_list(&mut self, gather: &mut Gather) -> Result<Progress, Toast> {
        gather.step(&mut self.vocab, &mut self.dict)
    }

    pub(super) fn vocab_act(&mut self, action: Download) {
        self.vocab.act(action);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn attach_vocab(&mut self, root: PathBuf) {
        let waker = Arc::clone(&self.waker);
        if let Err(why) = self.vocab.attach(root, waker) {
            self.emit(Event::Toast(Toast::error(why)));
            return;
        }
        self.vocab.set_inline(self.inline);
    }

    pub(super) fn pronunciation_act(&mut self, action: Download) {
        if let Err(why) = self.sounds.act(action) {
            self.emit(Event::Toast(Toast::error(why)));
        }
    }
}

impl Levels for Session {
    fn coming(&self) -> bool {
        self.vocab.coming()
    }

    fn ready(&self) -> bool {
        self.vocab.ready()
    }

    fn bands(&mut self, keys: &[String]) -> Vec<Option<u8>> {
        self.vocab.bands(keys)
    }
}

impl ShadingSource for Session {
    fn page(&mut self, doc: DocId, page: usize, layer: &Arc<TextLayer>, level: u8) -> Arc<Words> {
        self.vocab.page(doc, page, layer, level)
    }
}
