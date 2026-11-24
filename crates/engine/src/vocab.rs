//! Words above the reader's level, gathered page by page; see
//! `docs/study.md` § Vocabulary.
//!
//! What the front end draws of this is the shading on its sheets:
//! [`Vocab::page`] answers a page's hard words and the boxes each one runs
//! over, and the front end places those boxes on the sheets it is showing.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use mareader_cefr::{Part, Role, State, Ticket, Vocabulary};
use mareader_pdf::Bounds;
use mareader_session::{DocId, TextLayer};

use crate::dict::Download;
use crate::study::words;
use crate::study_mode::Tags;
use crate::word_list::Levels;

/// Shaded words per page at most; a book's stream page holds many.
const PAGE_CAP: usize = 5_000;
/// Pages remembered; past this the least recently used goes.
const MEMO_CAP: usize = 32;
/// Deck tag asks held at once; answered ones go first when full.
const TAG_CAP: usize = 1_024;

/// One page's hard words: each one's characters and its run of `boxes`.
#[derive(Default)]
pub struct Words {
    pub words: Vec<(Range<usize>, Range<usize>, u8)>,
    pub boxes: Vec<Bounds>,
}

/// One page's hard words and their boxes, while nothing changed.
struct Memo {
    layer: usize,
    level: u8,
    generation: u64,
    /// When it was last asked for, to let the stalest go.
    used: u64,
    words: Arc<Words>,
}

#[derive(Default)]
pub struct Vocab {
    service: Vocabulary,
    memo: HashMap<(DocId, usize), Memo>,
    /// Counts memo asks, to know the least recently used.
    asks: u64,
    /// The role asked for the open card, and its word, if any.
    role: Option<(Ticket, String)>,
    /// Study decks' tag asks: `None` while waiting, then the tag if any.
    tags: HashMap<Ticket, Option<Option<String>>>,
}

impl Vocab {
    /// Keeps the dataset in `dir`; `waker` repaints the app as work lands.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach(
        &mut self,
        dir: std::path::PathBuf,
        waker: mareader_cefr::Waker,
    ) -> Result<(), String> {
        self.memo.clear();
        self.service.attach(dir, waker)
    }

    /// Answers on this thread, as tests need.
    pub fn set_inline(&mut self, inline: bool) {
        self.service.set_inline(inline);
    }

    pub fn attached(&self) -> bool {
        self.service.attached()
    }

    pub fn state(&self, part: Part) -> &State {
        self.service.state(part)
    }

    pub fn words(&self) -> Option<u64> {
        self.service.words()
    }

    pub fn act(&mut self, action: Download) {
        #[cfg(not(target_arch = "wasm32"))]
        match action {
            Download::Start => self.service.download(),
            Download::Pause => self.service.pause(),
            Download::Cancel => self.service.cancel(),
            Download::Remove => self.service.remove(),
        }
        let _ = action;
    }

    /// Whether word levels can be asked for.
    pub fn ready(&self) -> bool {
        *self.service.state(Part::Dataset) == State::Ready
    }

    /// Whether word levels are on their way: fetching or converting.
    pub fn coming(&self) -> bool {
        let state = self.service.state(Part::Dataset);
        matches!(state, State::Fetching { .. } | State::Converting)
    }

    /// Each key's band where known; the rest are asked for.
    pub fn bands(&mut self, keys: &[String]) -> Vec<Option<u8>> {
        self.service.bands(keys)
    }

    /// Asks for the open card's word's role; a newer card's replaces it.
    ///
    /// False when no tagger is there to answer.
    pub fn ask_role(&mut self, word: &str, sentence: &str) -> bool {
        let ticket = self.service.role(word, sentence);
        self.role = ticket.map(|t| (t, word.to_owned()));
        self.role.is_some()
    }

    /// Takes in answers; the open card's word and its role once the
    /// tagger answered, none inside when it found none.
    pub fn poll(&mut self) -> (bool, Option<(String, Option<Role>)>) {
        let polled = self.service.poll();
        let mut role = None;
        for (ticket, answer) in polled.roles {
            if let Some(slot) = self.tags.get_mut(&ticket) {
                *slot = Some(answer.map(|r| r.pos));
                continue;
            }
            if let Some((asked, word)) = self.role.take() {
                match asked == ticket {
                    true => role = Some((word, answer)),
                    false => self.role = Some((asked, word)),
                }
            }
        }
        (polled.changed, role)
    }
}

impl Tags for Vocab {
    fn tagging(&self) -> bool {
        *self.service.state(Part::Tagger) == State::Ready
    }

    fn ask_tag(&mut self, word: &str, sentence: &str) -> Option<Ticket> {
        if self.tags.len() >= TAG_CAP {
            self.tags.retain(|_, tag| tag.is_none());
        }
        if self.tags.len() >= TAG_CAP {
            return None;
        }
        let ticket = self.service.role(word, sentence)?;
        self.tags.insert(ticket, None);
        Some(ticket)
    }

    fn take_tag(&mut self, ticket: Ticket) -> Option<Option<String>> {
        if matches!(self.tags.get(&ticket), Some(None)) {
            return None;
        }
        Some(self.tags.remove(&ticket).flatten().flatten())
    }
}

impl Vocab {
    /// `page`'s hard words, planned again only when something changed.
    pub fn page(
        &mut self,
        doc: DocId,
        page: usize,
        layer: &Arc<TextLayer>,
        level: u8,
    ) -> Arc<Words> {
        let id = Arc::as_ptr(layer).addr();
        let generation = self.service.generation();
        self.asks += 1;
        if let Some(m) = self.memo.get_mut(&(doc, page))
            && (m.layer, m.level, m.generation) == (id, level, generation)
        {
            m.used = self.asks;
            return Arc::clone(&m.words);
        }
        let (text, index) = words::read(layer);
        let spans = self.service.plan_banded_chars(&text, level, PAGE_CAP);
        let mut words = Words::default();
        for (s, band) in spans {
            let first = index.get(s.start).copied().flatten();
            let last = s
                .end
                .checked_sub(1)
                .and_then(|e| index.get(e).copied().flatten());
            let (Some(first), Some(last)) = (first, last) else {
                continue;
            };
            let start = words.boxes.len();
            words.boxes.extend(layer.text.line_boxes(first..last + 1));
            words
                .words
                .push((first..last + 1, start..words.boxes.len(), band));
        }
        words.words.shrink_to_fit();
        words.boxes.shrink_to_fit();
        let words = Arc::new(words);
        let stalest = (self.memo.len() >= MEMO_CAP)
            .then(|| self.memo.iter().min_by_key(|(_, m)| m.used))
            .flatten()
            .map(|(key, _)| *key);
        if let Some(key) = stalest {
            self.memo.remove(&key);
        }
        let memo = Memo {
            layer: id,
            level,
            generation,
            used: self.asks,
            words: Arc::clone(&words),
        };
        self.memo.insert((doc, page), memo);
        words
    }
}

/// Read-time source of hard words for page shading.
pub trait ShadingSource {
    /// `page`'s hard words, planned again only when something changed.
    fn page(&mut self, doc: DocId, page: usize, layer: &Arc<TextLayer>, level: u8) -> Arc<Words>;
}

impl ShadingSource for Vocab {
    fn page(&mut self, doc: DocId, page: usize, layer: &Arc<TextLayer>, level: u8) -> Arc<Words> {
        Vocab::page(self, doc, page, layer, level)
    }
}

/// The level cache answers what a word list asks for.
impl Levels for Vocab {
    fn coming(&self) -> bool {
        Vocab::coming(self)
    }

    fn ready(&self) -> bool {
        Vocab::ready(self)
    }

    fn bands(&mut self, keys: &[String]) -> Vec<Option<u8>> {
        Vocab::bands(self, keys)
    }
}
