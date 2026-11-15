//! Study tools on the page: the selection bar, marks and the word card;
//! see `docs/study.md`.

mod bar;
mod card;
pub mod marks;
mod note;
mod picking;
pub use picking::STYLE_ID as PICK_STYLE;
mod restyle;
pub mod words;

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Context, Pos2, Rect};
use mareader_appearance::Rgb;
use mareader_cefr::Role;
use mareader_library::Gloss as MarkGloss;
use mareader_library::{
    BookId, ContentId, DeviceId, Format, Library, Mark, MarkId, MarkKind, Measure, NewMark, Opened,
    Pen, Spot, Style,
};
use mareader_settings::{GlossSettings, HighlightSettings};
use mareader_study::{Explainer, Failure, Unavailable, WordInfo};
use mareader_workspace::PaneId;

pub use bar::{BarPress, show as show_bar};
pub use words::Asked;

use crate::command::{Command, Commands};
use crate::theme::Tokens;
use crate::toast::Toast;

/// How long Undo stays offered after a removal.
pub const UNDO_FOR: f64 = 6.0;

/// What a pane's pages report to the study tools this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Gloss {
    /// Explain was pressed on the pane's selection.
    Explain,
    /// Look up was pressed on the pane's selection.
    LookUp,
    /// Note was pressed on the pane's selection.
    Note,
    /// Read aloud was pressed: the selection alone is read.
    ReadAloud,
    /// The read-aloud pin rested, or was clicked: read from it.
    ReadFrom(crate::read_aloud::ReadFrom),
    /// The selection reached the clipboard.
    Copied,
    /// A mark was clicked: its card, beside it.
    Open {
        mark: MarkId,
        at: Rect,
    },
    /// A mark was right-clicked at `at`; `words` are its bounds.
    Menu {
        mark: MarkId,
        at: Pos2,
        words: Rect,
    },
    /// A highlighter in the selection bar; a colour if one was picked.
    Highlight(Pen, Option<Rgb>),
    /// Where the open card's mark is now; none while its text loads.
    Anchor(Option<Rect>),
    /// A long press on a mark: picking several begins with it.
    Pick(MarkId),
    /// A click on a mark while picking.
    Toggle(MarkId),
    /// While picking: the marks on screen, and every mark of the book.
    Seen {
        page: Vec<MarkId>,
        all: Vec<MarkId>,
    },
    /// A shaded word was clicked: its characters on `page`.
    Shaded {
        page: usize,
        chars: std::ops::Range<usize>,
        at: Rect,
    },
    /// The pointer rested on a shaded word, in its sentence.
    Rest {
        word: String,
        context: String,
        at: Rect,
    },
    /// A hold on empty space lifted the pane, to be moved.
    Lift(Pos2),
    /// A right-click on a page, off its marks and selection.
    PageMenu {
        page: usize,
        at: Pos2,
        /// The book's text under the pointer, if any.
        place: Option<mareader_session::Anchor>,
        /// The character under the pointer, on a line of text.
        caret: Option<usize>,
    },
    /// A page's AI bubble was clicked, or right-clicked.
    Bubble {
        page: usize,
        index: usize,
        at: Pos2,
    },
    BubbleMenu {
        page: usize,
        index: usize,
        at: Pos2,
    },
    /// The continuous run's pill was pressed.
    Pill(crate::page_ai::PillPress),
}

/// What the card and the mark menu ask of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Remove(Vec<MarkId>),
    /// Undo: the removed marks come back.
    Restore(Vec<MarkId>),
    /// Asks again after a failure.
    Retry,
    /// Asks the AI chosen now; its answer replaces the saved one.
    AskAgain,
    Close,
    /// The mark menu's Select several: picking begins with its mark.
    SelectSeveral(PaneId, MarkId),
    /// Picks the marks on screen, or every mark of the book.
    PickPage,
    PickAll,
    DonePicking,
    /// The mark menu's Add note or Edit note.
    WriteNote(MarkId),
    /// The notes panel's: a draft in `PaneId`, placed once its words show.
    WriteNoteIn(PaneId, MarkId),
    /// The open note's Edit: its text becomes a draft.
    EditNote,
    /// Keeps the draft on `MarkId`'s note; blank deletes it.
    SaveNote(MarkId),
    CancelNote,
    DeleteNote(MarkId),
    /// Undo: a deleted note's text comes back.
    RestoreNote(MarkId, String),
    /// A highlight's new pen and colour, from its menu.
    Restyle(MarkId, Style),
    /// Picked highlights' new looks, and how many couldn't take it.
    RestyleAll(Vec<(MarkId, Style)>, usize),
    /// Undo: the picked highlights' looks before.
    Unstyle(Vec<(MarkId, Style)>),
    /// Shots: types into the open draft, then saves it if asked.
    TypeNote(String, bool),
}

/// Which answer a card shows: a mark's, or one kept by nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Mark(MarkId),
    Loose(u64),
}

#[derive(Clone, Debug, PartialEq)]
struct Card {
    pane: PaneId,
    key: Key,
    word: String,
    context: String,
    /// The word on screen, as of the last frame it was seen.
    at: Rect,
    /// Seen on screen this frame; a mark scrolled away closes it.
    seen: bool,
    /// The word's role in its sentence, once the tagger said.
    role: Option<Role>,
}

crate::word_card::impl_card!(Card);

/// The right-click menu on a mark.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MarkMenu {
    pane: PaneId,
    mark: MarkId,
    at: Pos2,
    /// The mark's words on screen, for a note opened from here.
    rect: Rect,
    /// A highlight's: whether it has a note; none for an AI mark.
    noted: Option<bool>,
    /// A highlight's look, to change; none for an AI mark.
    style: Option<Style>,
    /// Its words were drawn this frame: it follows them, else goes.
    seen: bool,
}

/// A highlight's note beside it: read, or being written.
#[derive(Clone, Debug, PartialEq)]
struct Note {
    pane: PaneId,
    /// The mark as it was opened, its note kept up to date here.
    mark: Mark,
    at: Rect,
    seen: bool,
    /// The text being written; none while the note is only read.
    draft: Option<String>,
    /// The highlight was made for this note: Cancel takes it back.
    placeholder: bool,
    /// Frames left to give the field focus, as the card measures first.
    focus: u8,
}

/// What a worker sends back: the answer so far, then the end.
enum Got {
    Partial(WordInfo),
    Done(Result<Said, Failure>),
}

/// A finished answer and who gave it.
#[derive(Clone, Debug, PartialEq)]
struct Said {
    info: WordInfo,
    by: String,
}

impl Said {
    fn of(mark: &Mark) -> Option<Self> {
        let g = mark.gloss.value.as_ref()?;
        let info = WordInfo {
            pos: g.pos.clone(),
            meaning: g.meaning.clone(),
            synonyms: g.synonyms.clone(),
            usages: g.usages.clone(),
        };
        Some(Self {
            info,
            by: g.by.clone(),
        })
    }

    fn gloss(&self) -> MarkGloss {
        MarkGloss {
            pos: self.info.pos.clone(),
            meaning: self.info.meaning.clone(),
            synonyms: self.info.synonyms.clone(),
            usages: self.info.usages.clone(),
            by: self.by.clone(),
        }
    }
}

/// Answers, the open card and menu, and marks kept by no library.
pub struct Study {
    explainer: Arc<dyn Explainer>,
    /// Answers on this thread, as tests and the browser need.
    inline: bool,
    /// Documents outside the library keep marks here, for the session.
    loose: Library,
    card: Option<Card>,
    menu: Option<MarkMenu>,
    note: Option<Note>,
    /// Marks picked to remove together, in one pane.
    picking: Option<picking::Picking>,
    /// Answers this session; a mark's are saved with it too.
    answers: HashMap<Key, Said>,
    /// Answers still arriving, as far as they got.
    partial: HashMap<Key, WordInfo>,
    failed: HashMap<Key, Failure>,
    asking: HashMap<Key, Receiver<Got>>,
    /// Finished answers not yet filed, as inline asks leave them.
    done: Vec<(Key, Said)>,
    next_loose: u64,
    /// Last frame's card and menu, so a press outside closes them.
    card_rect: Option<Rect>,
    menu_rect: Option<Rect>,
    note_rect: Option<Rect>,
    /// Books' marks as last listed: every pane asks each frame.
    listed: RefCell<Vec<Listed>>,
}

/// A book's bookmarks and where each is.
pub type Bookmarks = Arc<[(MarkId, Spot)]>;

/// Books listed at most; a ninth lets the oldest go.
const LISTED_CAP: usize = 8;

/// A book's marks and bookmarks, while none changed and the look held.
struct Listed {
    stamp: u64,
    book: BookId,
    look: HighlightSettings,
    shown: Arc<[marks::Shown]>,
    bookmarks: Bookmarks,
}

impl Default for Study {
    fn default() -> Self {
        Self {
            explainer: Arc::new(Unavailable),
            inline: false,
            loose: Library::new(DeviceId(0)),
            card: None,
            menu: None,
            note: None,
            picking: None,
            answers: HashMap::new(),
            partial: HashMap::new(),
            failed: HashMap::new(),
            asking: HashMap::new(),
            done: Vec::new(),
            next_loose: 0,
            card_rect: None,
            menu_rect: None,
            note_rect: None,
            listed: RefCell::default(),
        }
    }
}

/// Records session document `name` in `loose`, where its marks are kept.
pub(crate) fn loose_book(loose: &mut Library, name: &str) -> BookId {
    let opened = Opened {
        path: PathBuf::from(format!("session:{name}")),
        measure: Measure {
            content: loose_content(name),
            stamp: Default::default(),
        },
        format: Format::Pdf,
        title: name.to_owned(),
        author: None,
    };
    loose.record_open(opened, 0)
}

/// Session document `name`'s id in a loose library.
pub(crate) fn loose_id(name: &str) -> BookId {
    BookId::original(loose_content(name))
}

/// A document outside the library, known by its name for the session.
fn loose_content(name: &str) -> ContentId {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    ContentId(u128::from(h.finish()))
}

impl Study {
    /// True while someone answers for the AI Dictionary.
    pub fn explains(&self) -> bool {
        !self.explainer.by().is_empty()
    }

    pub fn set_explainer(&mut self, explainer: Arc<dyn Explainer>) {
        // Saved answers stay; Ask again reaches the new AI.
        self.explainer = explainer;
        self.failed.clear();
    }

    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
    }

    pub fn inline(&self) -> bool {
        self.inline
    }

    /// The mark whose card or note is open in `pane`.
    pub fn card_mark(&self, pane: PaneId) -> Option<MarkId> {
        if let Some(n) = self.note.as_ref().filter(|n| n.pane == pane) {
            return Some(n.mark.id);
        }
        if let Some(c) = self.card.as_ref().filter(|c| c.pane == pane) {
            return match c.key {
                Key::Mark(id) => Some(id),
                Key::Loose(_) => None,
            };
        }
        self.menu.filter(|m| m.pane == pane).map(|m| m.mark)
    }

    /// Where the mark menu was drawn, for tests.
    pub fn menu_rect(&self) -> Option<Rect> {
        self.menu.and(self.menu_rect)
    }

    pub fn card_open(&self) -> bool {
        self.card.is_some()
    }

    /// The open note's mark, and its draft while written; for tests.
    pub fn note_open(&self) -> Option<(MarkId, Option<&str>)> {
        let n = self.note.as_ref()?;
        Some((n.mark.id, n.draft.as_deref()))
    }

    /// The word the open card explains.
    pub fn card_word(&self) -> Option<&str> {
        self.card.as_ref().map(|c| c.word.as_str())
    }

    /// The open card's word and context, to ask for its role.
    pub fn card_words(&self) -> Option<(&str, &str)> {
        let c = self.card.as_ref()?;
        Some((c.word.as_str(), c.context.as_str()))
    }

    /// The role the open card shows, for tests.
    pub fn card_role(&self) -> Option<&Role> {
        self.card.as_ref()?.role.as_ref()
    }

    /// The tagger's answer for the open card, if it is still `word`'s.
    pub fn set_role(&mut self, word: &str, role: Role) {
        crate::word_card::set_role(&mut self.card, word, role);
    }

    /// The open card's answer, once it came, for tests.
    pub fn card_answer(&self) -> Option<&WordInfo> {
        self.answers.get(&self.card.as_ref()?.key).map(|s| &s.info)
    }

    /// Who gave the open card's answer, for tests.
    pub fn card_by(&self) -> Option<&str> {
        self.answers
            .get(&self.card.as_ref()?.key)
            .map(|s| s.by.as_str())
    }

    /// The book's library: the app's, else the session's for `name`.
    fn shelf<'a>(
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

    /// `book`'s marks, else the session document `name`'s; AI marks last.
    /// Bookmarks hang ribbons instead.
    pub fn marks_of(
        &self,
        main: &Library,
        book: Option<BookId>,
        name: &str,
        look: &HighlightSettings,
    ) -> Arc<[marks::Shown]> {
        self.listed(main, book, name, look).0
    }

    /// `book`'s marks to draw and its bookmarks, listed again on a change.
    fn listed(
        &self,
        main: &Library,
        book: Option<BookId>,
        name: &str,
        look: &HighlightSettings,
    ) -> (Arc<[marks::Shown]>, Bookmarks) {
        let (lib, id) = self.lib_of(main, book, name);
        let stamp = lib.marks_stamp();
        let mut listed = self.listed.borrow_mut();
        let same = |l: &&Listed| l.stamp == stamp && l.book == id && l.look == *look;
        if let Some(l) = listed.iter().find(same) {
            return (Arc::clone(&l.shown), Arc::clone(&l.bookmarks));
        }
        let all = lib.marks(id);
        let (kept, drawn): (Vec<&Mark>, Vec<&Mark>) =
            (all.into_iter()).partition(|m| m.kind == MarkKind::Bookmark);
        let mut shown: Vec<marks::Shown> = (drawn.into_iter())
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

    /// `book`'s bookmarks, each with where it is.
    pub fn bookmarks_of(&self, main: &Library, book: Option<BookId>, name: &str) -> Bookmarks {
        let (lib, id) = self.lib_of(main, book, name);
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

    /// The library keeping `book`'s marks, and its id there.
    fn lib_of<'a>(
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

    /// Bookmarks the page, or takes back its bookmark with Undo offered.
    pub fn toggle_bookmark(
        &mut self,
        main: &mut Library,
        at: Bookmarking<'_>,
        wall_ms: u64,
        out: &mut Commands,
    ) {
        if let Some(id) = at.existing {
            self.act_remove(main, vec![id], wall_ms, out);
            return;
        }
        let (lib, book) = self.shelf(main, at.book, at.name);
        lib.add_mark(book, NewMark::bookmark(at.label, at.spot), wall_ms);
    }

    /// The book's highlights and bookmarks for the panel, in reading order.
    pub fn notes_of(
        &self,
        main: &Library,
        book: Option<BookId>,
        name: &str,
        look: &HighlightSettings,
    ) -> Vec<crate::notes_panel::Row> {
        let (lib, id) = match book.filter(|b| main.book(*b).is_some()) {
            Some(id) => (main, id),
            None => (&self.loose, BookId::original(loose_content(name))),
        };
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

    /// The reader's highlighter: marks the text in `pen`; the mark and
    /// whether it is new.
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

    /// Note pressed: highlights the text and opens a draft on it.
    pub fn write_note(
        &mut self,
        main: &mut Library,
        pane: PaneId,
        at: Highlighted<'_>,
        rect: Rect,
        wall_ms: u64,
    ) {
        let Some((id, made)) = self.highlight(main, at, wall_ms) else {
            return;
        };
        self.open_note(main, pane, id, rect, true);
        if let Some(n) = &mut self.note {
            n.placeholder = made;
        }
    }

    /// Opens `id`'s note beside `at`: a draft if `write`, else to read.
    fn open_note(&mut self, main: &Library, pane: PaneId, id: MarkId, at: Rect, write: bool) {
        let Some(mark) = self.mark(main, id).cloned() else {
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

    fn mark<'a>(&'a self, main: &'a Library, id: MarkId) -> Option<&'a Mark> {
        main.mark(id).or_else(|| self.loose.mark(id))
    }

    /// Explain pressed: marks the word if it can and opens its card.
    pub fn explain(&mut self, ctx: &Context, main: &mut Library, at: Explained<'_>, wall_ms: u64) {
        let Explained {
            pane,
            book,
            name,
            asked,
            rect,
        } = at;
        let new = asked
            .spot
            .map(|spot| NewMark::explain(asked.word.clone(), asked.context.clone(), spot));
        let (lib, book) = self.shelf(main, book, name);
        let key = match new.and_then(|n| lib.add_mark(book, n, wall_ms)) {
            Some(id) => Key::Mark(id),
            None => {
                self.next_loose += 1;
                Key::Loose(self.next_loose)
            }
        };
        self.open(ctx, main, pane, key, (asked.word, asked.context), rect);
    }

    /// A click on a mark: its card again; a highlight's note or menu.
    pub fn open_mark(&mut self, ctx: &Context, main: &Library, pane: PaneId, id: MarkId, at: Rect) {
        let Some(m) = self.mark(main, id) else {
            return;
        };
        if m.kind == MarkKind::Highlight && m.note.value.is_some() {
            self.open_note(main, pane, id, at, false);
            return;
        }
        if m.kind == MarkKind::Highlight {
            let p = ctx.pointer_latest_pos().unwrap_or(at.left_bottom());
            self.menu_at(main, pane, id, p, at);
            return;
        }
        let words = (m.word.clone(), m.context.clone());
        self.open(ctx, main, pane, Key::Mark(id), words, at);
    }

    fn open(
        &mut self,
        ctx: &Context,
        main: &Library,
        pane: PaneId,
        key: Key,
        words: (String, String),
        at: Rect,
    ) {
        let (word, context) = words;
        self.menu = None;
        self.note = None;
        // A failed answer is asked for again; a saved one is shown.
        self.failed.remove(&key);
        if let Key::Mark(id) = key
            && !self.answers.contains_key(&key)
            && let Some(said) = self.mark(main, id).and_then(Said::of)
        {
            self.answers.insert(key, said);
        }
        if !self.answers.contains_key(&key) && !self.asking.contains_key(&key) {
            self.ask(ctx, key, &word, &context);
        }
        self.card = Some(Card {
            pane,
            key,
            word,
            context,
            at,
            seen: true,
            role: None,
        });
    }

    fn ask(&mut self, ctx: &Context, key: Key, word: &str, context: &str) {
        let explainer = Arc::clone(&self.explainer);
        let by = explainer.by();
        self.partial.remove(&key);
        if self.inline || cfg!(target_arch = "wasm32") {
            match explainer.explain(word, context, &|_| {}) {
                Ok(info) => {
                    let info = info.clamped();
                    self.done.push((key, Said { info, by }));
                }
                Err(e) => {
                    self.failed.insert(key, e);
                }
            }
            return;
        }
        let (tx, rx) = channel();
        let (word, context, ctx) = (word.to_owned(), context.to_owned(), ctx.clone());
        std::thread::spawn(move || {
            let partial = |info| {
                let _ = tx.send(Got::Partial(info));
                ctx.request_repaint();
            };
            let done = explainer.explain(&word, &context, &partial);
            let said = done.map(|i| Said {
                info: i.clamped(),
                by,
            });
            let _ = tx.send(Got::Done(said));
            ctx.request_repaint();
        });
        self.asking.insert(key, rx);
    }

    /// The right-click menu on a mark.
    pub fn open_menu(&mut self, main: &Library, pane: PaneId, mark: MarkId, at: Pos2, words: Rect) {
        self.menu_at(main, pane, mark, at, words);
    }

    fn menu_at(&mut self, main: &Library, pane: PaneId, mark: MarkId, at: Pos2, rect: Rect) {
        let highlight = self
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

    /// `pane` drew no pages this frame, as while it re-sets: the card stays.
    pub fn hold(&mut self, pane: PaneId) {
        if let Some(card) = self.card.as_mut().filter(|c| c.pane == pane) {
            card.seen = true;
        }
        if let Some(note) = self.note.as_mut().filter(|n| n.pane == pane) {
            note.seen = true;
        }
        if let Some(menu) = self.menu.as_mut().filter(|m| m.pane == pane) {
            menu.seen = true;
        }
    }

    /// Escape: drops a draft, closes the card or menu, else stops picking.
    pub fn escape(&mut self, main: &mut Library) -> bool {
        if self.note.is_some() {
            self.cancel_note(main);
            return true;
        }
        if self.card.is_some() || self.menu.is_some() {
            self.card = None;
            self.menu = None;
            return true;
        }
        self.picking.take().is_some()
    }

    /// Takes the card's and menu's asks; marks change in the libraries.
    pub fn act(&mut self, ctx: &Context, main: &mut Library, action: Action, out: &mut Commands) {
        let wall = crate::library::wall_ms();
        match action {
            Action::Remove(ids) => self.act_remove(main, ids, wall, out),
            Action::Restore(ids) => {
                main.set_marks_alive(&ids, true, wall);
                self.loose.set_marks_alive(&ids, true, wall);
            }
            Action::Retry => {
                if let Some(card) = self.card.clone() {
                    self.failed.remove(&card.key);
                    self.ask(ctx, card.key, &card.word, &card.context);
                }
            }
            Action::AskAgain => {
                // The saved answer stays until a new one replaces it.
                if let Some(card) = self.card.clone()
                    && !self.asking.contains_key(&card.key)
                {
                    self.answers.remove(&card.key);
                    self.failed.remove(&card.key);
                    self.ask(ctx, card.key, &card.word, &card.context);
                }
                self.save(main);
            }
            Action::Close => {
                self.card = None;
                self.menu = None;
            }
            Action::SelectSeveral(pane, mark) => self.pick(pane, mark),
            Action::Restyle(id, style) => {
                if !main.set_style(id, style, wall) {
                    self.loose.set_style(id, style, wall);
                }
                // The menu stays, showing the new look.
                if let Some(m) = self.menu.as_mut().filter(|m| m.mark == id) {
                    m.style = Some(style);
                }
            }
            Action::RestyleAll(changes, skipped) => {
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
                let toast = match n {
                    0 => Toast::info(message),
                    _ => Toast::info(message).undo(Command::Study(Action::Unstyle(before))),
                };
                out.push(Command::Notify(toast));
            }
            Action::Unstyle(looks) => {
                for (id, style) in looks {
                    if !main.set_style(id, style, wall) {
                        self.loose.set_style(id, style, wall);
                    }
                }
            }
            Action::PickPage | Action::PickAll => {
                if let Some(p) = &mut self.picking {
                    let more = match action {
                        Action::PickPage => p.page.clone(),
                        _ => p.all.clone(),
                    };
                    p.add(&more);
                }
            }
            Action::DonePicking => self.picking = None,
            Action::WriteNote(id) => {
                if let Some(m) = self.menu.take() {
                    self.open_note(main, m.pane, id, m.rect, true);
                }
            }
            Action::WriteNoteIn(pane, id) => {
                self.open_note(main, pane, id, Rect::NOTHING, true);
            }
            Action::EditNote => {
                if let Some(n) = &mut self.note {
                    n.draft = Some(n.mark.note.value.clone().unwrap_or_default());
                    n.focus = 2;
                }
            }
            Action::SaveNote(id) => self.save_note(main, id, out),
            Action::CancelNote => self.cancel_note(main),
            Action::DeleteNote(id) => {
                self.note = None;
                self.forget_note(main, id, out);
            }
            Action::TypeNote(text, save) => {
                let Some(n) = self.note.as_mut() else {
                    return;
                };
                n.draft = Some(text);
                if save {
                    let id = n.mark.id;
                    self.save_note(main, id, out);
                }
            }
            Action::RestoreNote(id, text) => {
                if !main.set_note(id, Some(text.clone()), wall) {
                    self.loose.set_note(id, Some(text), wall);
                }
            }
        }
    }

    /// Removes marks, saying which kind went, with Undo.
    fn act_remove(&mut self, main: &mut Library, ids: Vec<MarkId>, wall: u64, out: &mut Commands) {
        let bookmark = |id: &MarkId| {
            let mark = self.mark(main, *id);
            mark.is_some_and(|m| m.kind == MarkKind::Bookmark)
        };
        let what = if ids.iter().all(bookmark) {
            "bookmark"
        } else {
            "highlight"
        };
        let n =
            main.set_marks_alive(&ids, false, wall) + self.loose.set_marks_alive(&ids, false, wall);
        let gone = |c: &Card| matches!(c.key, Key::Mark(id) if ids.contains(&id));
        if self.card.as_ref().is_some_and(gone) {
            self.card = None;
        }
        self.menu = None;
        self.picking = None;
        if n > 0 {
            let message = match n {
                1 => format!("Removed {what}"),
                n => format!("Removed {n} {what}s"),
            };
            let undo = Command::Study(Action::Restore(ids));
            out.push(Command::Notify(Toast::info(message).undo(undo)));
        }
    }

    /// Keeps the open draft; blank deletes the note or its new highlight.
    fn save_note(&mut self, main: &mut Library, id: MarkId, out: &mut Commands) {
        let Some(n) = self.note.take_if(|n| n.mark.id == id) else {
            return;
        };
        let Some(draft) = n.draft else {
            return;
        };
        let text = draft.trim();
        if text.is_empty() {
            if n.placeholder {
                self.drop_marks(main, &[id]);
            } else if n.mark.note.value.is_some() {
                self.forget_note(main, id, out);
            }
            return;
        }
        let wall = crate::library::wall_ms();
        if !main.set_note(id, Some(text.to_owned()), wall) {
            self.loose.set_note(id, Some(text.to_owned()), wall);
        }
    }

    /// Closes the note unsaved; a highlight made for it goes too.
    fn cancel_note(&mut self, main: &mut Library) {
        if let Some(n) = self.note.take().filter(|n| n.placeholder) {
            self.drop_marks(main, &[n.mark.id]);
        }
    }

    /// Deletes `id`'s note, with Undo bringing its text back.
    fn forget_note(&mut self, main: &mut Library, id: MarkId, out: &mut Commands) {
        let was = self.mark(main, id).and_then(|m| m.note.value.clone());
        let wall = crate::library::wall_ms();
        if !main.set_note(id, None, wall) {
            self.loose.set_note(id, None, wall);
        }
        if let Some(text) = was {
            let undo = Command::Study(Action::RestoreNote(id, text));
            out.push(Command::Notify(Toast::info("Deleted note").undo(undo)));
        }
    }

    /// Removes marks quietly, as a cancelled note's highlight.
    fn drop_marks(&mut self, main: &mut Library, ids: &[MarkId]) {
        let wall = crate::library::wall_ms();
        main.set_marks_alive(ids, false, wall);
        self.loose.set_marks_alive(ids, false, wall);
    }

    /// Takes in answers, finished or still filling in; marks keep theirs.
    pub fn receive(&mut self, main: &mut Library) {
        let mut done = Vec::new();
        for (key, rx) in &self.asking {
            loop {
                match rx.try_recv() {
                    Ok(Got::Partial(info)) => {
                        self.partial.insert(*key, info.clamped());
                    }
                    Ok(Got::Done(answer)) => {
                        done.push((*key, Some(answer)));
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        done.push((*key, None));
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
    }

    /// Files finished answers: in the session, and with their marks.
    fn save(&mut self, main: &mut Library) {
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

    /// Before the panes draw: marks must show themselves again.
    pub fn begin_frame(&mut self) {
        if let Some(card) = &mut self.card {
            card.seen = matches!(card.key, Key::Loose(_));
        }
        // A draft stays while its words scroll away; a read note goes.
        if let Some(note) = &mut self.note {
            note.seen = note.draft.is_some();
        }
        if let Some(menu) = &mut self.menu {
            menu.seen = false;
        }
    }

    /// After the panes: the card and the mark menu, over them.
    pub fn show(
        &mut self,
        ctx: &Context,
        main: &Library,
        (panes, heard): (&[(PaneId, Rect)], crate::sounds::Heard<'_>),
        (look, pens): (&GlossSettings, &HighlightSettings),
        out: &mut Commands,
    ) {
        let pressed = ctx.input(|i| {
            let p = i.pointer.press_origin();
            p.filter(|_| i.pointer.any_pressed())
        });
        let outside = |r: Option<Rect>| pressed.is_some_and(|p| !r.is_some_and(|r| r.contains(p)));
        if self.card.is_some() && outside(self.card_rect) {
            self.card = None;
        }
        let room = |pane| panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r);
        // A press elsewhere, or its words scrolled away, closes the menu.
        let gone = |m: &MarkMenu| !m.seen || room(m.pane).is_none();
        if outside(self.menu_rect) || self.menu.as_ref().is_some_and(gone) {
            self.menu = None;
        }
        // A press elsewhere keeps a draft; a note only read just closes.
        if let Some(n) = &self.note
            && (outside(self.note_rect) || room(n.pane).is_none() || !n.seen)
        {
            match n.draft {
                Some(_) => out.push(Command::Study(Action::SaveNote(n.mark.id))),
                None => self.note = None,
            }
        }
        // A mark that scrolled away, or a pane that closed, takes its card.
        if self
            .card
            .as_ref()
            .is_some_and(|c| !c.seen || room(c.pane).is_none())
        {
            self.card = None;
        }
        let t = Tokens::get(ctx);
        self.card_rect = self.card.as_ref().and_then(|c| {
            let answer = match (self.answers.get(&c.key), self.failed.get(&c.key)) {
                (Some(said), _) => card::Answer::Ready(&said.info, &said.by),
                (None, Some(e)) => card::Answer::Failed(e),
                (None, None) => match self.partial.get(&c.key) {
                    Some(info) => card::Answer::Partial(info),
                    None => card::Answer::Waiting,
                },
            };
            let view = card::View {
                word: &c.word,
                role: c.role.as_ref(),
                answer,
                at: c.at,
                room: room(c.pane)?,
                density: look.density,
                saying: heard.saying(&c.word),
                sound: heard.lines(&c.word),
            };
            Some(card::show(ctx, &view, &t, out))
        });
        self.menu_rect = self.menu.map(|m| {
            card::menu(
                ctx,
                (m.pane, m.mark),
                (m.at, m.rect),
                m.noted,
                (m.style, pens),
                &t,
                out,
            )
        });
        self.note_rect = match &mut self.note {
            Some(n) => room(n.pane).map(|room| {
                // Opened from the panel, before its words were placed.
                let at = match n.at.is_finite() {
                    true => n.at,
                    false => Rect::from_center_size(room.center(), egui::Vec2::splat(1.0)),
                };
                let view = note::View {
                    mark: n.mark.id,
                    quote: &n.mark.word,
                    ink: ink(&n.mark, pens).color,
                    saved: n.mark.note.value.as_deref(),
                    draft: n.draft.as_mut(),
                    fresh: n.focus > 0,
                    at,
                    room,
                };
                n.focus = n.focus.saturating_sub(1);
                note::show(ctx, view, &t, out)
            }),
            None => None,
        };
        if self
            .picking
            .as_ref()
            .is_some_and(|p| room(p.pane).is_none())
        {
            self.picking = None;
        }
        if let Some(p) = &self.picking
            && let Some(r) = room(p.pane)
        {
            // The highlights picked, with their looks; AI marks keep theirs.
            let ids = p.ids.iter().filter_map(|id| self.mark(main, *id));
            let words = ids.filter(|m| m.kind == MarkKind::Highlight);
            let looks: Vec<_> = words.map(|m| (m.id, m.look())).collect();
            picking::bar(ctx, p, (r, &looks, pens), &t, out);
        }
    }
}

/// Explain's ask, as the app gathers it.
pub struct Explained<'a> {
    pub pane: PaneId,
    pub book: Option<BookId>,
    /// The document's name: a session's marks are kept by it.
    pub name: &'a str,
    pub asked: Asked,
    /// The word on screen.
    pub rect: Rect,
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

/// How `m` is drawn; a highlight never takes a reserved look.
fn ink(m: &Mark, look: &HighlightSettings) -> marks::Ink {
    if m.kind == MarkKind::Explain {
        return marks::Ink {
            pen: look.ai.pen,
            color: look.ai.color,
            ai: true,
        };
    }
    let style = m.look();
    let color = restyle::shown(style, look);
    marks::Ink {
        pen: style.pen,
        color,
        ai: false,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use mareader_study::Canned;

    use super::*;

    fn marked(said: &str) -> (Library, MarkId) {
        let mut main = Library::new(DeviceId(1));
        let opened = Opened {
            path: PathBuf::from("/a.pdf"),
            measure: Measure {
                content: ContentId(7),
                stamp: Default::default(),
            },
            format: Format::Pdf,
            title: "A".into(),
            author: None,
        };
        let book = main.record_open(opened, 1);
        let spot = Spot::Page {
            page: 0,
            rect: [1.0, 2.0, 3.0, 4.0],
        };
        let new = NewMark::explain("quick".into(), "the quick fox".into(), spot);
        let id = main.add_mark(book, new, 2).unwrap();
        let gloss = MarkGloss {
            meaning: said.into(),
            by: "Ollama · gemma".into(),
            ..MarkGloss::default()
        };
        main.set_gloss(id, Some(gloss), 3);
        (main, id)
    }

    #[test]
    fn a_saved_answer_shows_without_asking_and_ask_again_replaces_it() {
        let (mut main, id) = marked("Fast.");
        let ctx = Context::default();
        // A fresh session whose AI can't answer: the saved answer shows.
        let mut study = Study::default();
        study.set_inline(true);
        study.open_mark(&ctx, &main, PaneId(1), id, Rect::ZERO);
        let meaning = study.card_answer().map(|a| a.meaning.clone());
        assert_eq!(meaning.as_deref(), Some("Fast."));
        assert_eq!(study.card_by(), Some("Ollama · gemma"));

        study.set_explainer(Arc::new(Canned));
        let mut out = Commands::new();
        study.act(&ctx, &mut main, Action::AskAgain, &mut out);
        assert_eq!(study.card_by(), Some("Sample answers"));
        let saved = main.mark(id).and_then(|m| m.gloss.value.clone()).unwrap();
        assert_eq!(saved.by, "Sample answers");
        assert_ne!(saved.meaning, "Fast.");
    }

    #[test]
    fn listed_marks_are_shared_until_one_changes() {
        let (mut main, id) = marked("Fast.");
        let study = Study::default();
        let look = HighlightSettings::default();
        let book = main.mark(id).map(|m| m.book.value);
        let first = study.marks_of(&main, book, "", &look);
        let again = study.marks_of(&main, book, "", &look);
        assert!(
            Arc::ptr_eq(&first, &again),
            "an unchanged book is not listed again"
        );
        assert!(main.set_note(id, Some("a note".into()), 4));
        let noted = study.marks_of(&main, book, "", &look);
        assert!(
            noted.first().is_some_and(|m| m.noted),
            "a change shows at once"
        );
        main.set_marks_alive(&[id], false, 5);
        assert!(study.marks_of(&main, book, "", &look).is_empty());
        assert!(study.bookmarks_of(&main, book, "").is_empty());
    }
}
