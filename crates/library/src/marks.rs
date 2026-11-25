//! Marks: words explained and text highlighted in a book (M6).

use std::hash::{BuildHasher, RandomState};

use serde::{Deserialize, Serialize};

use crate::clock::{Field, Stamp};
use crate::extra::Extra;
use crate::id::{BookId, MarkId};
use crate::library::Library;

/// Where a marked word stands; see `docs/study.md` section Marks.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spot {
    /// A PDF's page and the word's box: left, top, width, height.
    Page { page: usize, rect: [f32; 4] },
    /// A book's block and the word's bytes in its text.
    Text {
        block: usize,
        start: usize,
        end: usize,
    },
    /// A PDF's text from `[page, char]` up to `[page, char]`, any pages.
    Chars { from: [usize; 2], to: [usize; 2] },
    /// A book's text from `[block, byte]` up to `[block, byte]`.
    Blocks { from: [usize; 2], to: [usize; 2] },
}

/// How a highlight is drawn; see `docs/study.md` section Highlighters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pen {
    /// A fill over the whole line.
    #[default]
    Full,
    /// A fill over the line's lower half.
    Half,
    Underline,
}

impl Pen {
    pub const ALL: [Pen; 3] = [Pen::Full, Pen::Half, Pen::Underline];

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Highlight",
            Self::Half => "Half highlight",
            Self::Underline => "Underline",
        }
    }
}

/// What made a mark.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkKind {
    /// Explain: one word, drawn in the AI's reserved look.
    #[default]
    Explain,
    /// The reader's own highlighter, over any text.
    Highlight,
    /// A page kept to come back to; drawn as a ribbon, not over text.
    Bookmark,
}

impl Spot {
    /// The same place, give or take a point of a PDF's box.
    pub fn near(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Page { page: a, rect: r }, Self::Page { page: b, rect: s }) => {
                let close = |i: usize| match (r.get(i), s.get(i)) {
                    (Some(x), Some(y)) => (x - y).abs() < 1.0,
                    _ => false,
                };
                a == b && close(0) && close(1)
            }
            _ => self == other,
        }
    }
}

/// What the AI said about a marked word, and who said it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gloss {
    pub pos: String,
    pub meaning: String,
    pub synonyms: Vec<String>,
    pub usages: Vec<String>,
    /// The provider and model, as the card's small print names them.
    pub by: String,
}

/// One marked word. Only `book`, `alive`, `gloss` and `note` change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    pub id: MarkId,
    /// Stamped: a merge that folds two books moves their marks.
    pub book: Field<BookId>,
    #[serde(default)]
    pub kind: MarkKind,
    /// A highlight's pen as made; `look` has it after any restyle.
    #[serde(default)]
    pub pen: Pen,
    /// Its colour as made, as sRGB; none follows the pen's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    /// The word, or a highlight's text.
    pub word: String,
    /// The words around it, as sent to be explained.
    pub context: String,
    pub spot: Spot,
    /// When it was made, in wall milliseconds.
    pub added: u64,
    /// Removing and Undo merge like a shelf's membership.
    pub alive: Field<bool>,
    /// The saved answer; it hides and returns with the mark.
    #[serde(default, skip_serializing_if = "Field::is_unset")]
    pub gloss: Field<Option<Gloss>>,
    /// The reader's own words about a highlight.
    #[serde(default, skip_serializing_if = "Field::is_unset")]
    pub note: Field<Option<String>>,
    /// A restyle, stamped so the latest wins over the look as made.
    #[serde(default, skip_serializing_if = "Field::is_unset")]
    pub style: Field<Option<Style>>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Mark {
    /// How it is drawn: the latest restyle, else the look it was made with.
    pub fn look(&self) -> Style {
        let made = Style {
            pen: self.pen,
            color: self.color,
        };
        self.style.value.unwrap_or(made)
    }
}

/// A highlight's look after a restyle: its pen, and a colour or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    pub pen: Pen,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
}

/// A new mark's word, context and place.
#[derive(Clone, Debug, PartialEq)]
pub struct NewMark {
    pub kind: MarkKind,
    pub pen: Pen,
    pub color: Option<[u8; 3]>,
    pub word: String,
    pub context: String,
    pub spot: Spot,
}

impl NewMark {
    /// A word Explain marks.
    pub fn explain(word: String, context: String, spot: Spot) -> Self {
        Self {
            kind: MarkKind::Explain,
            pen: Pen::Full,
            color: None,
            word,
            context,
            spot,
        }
    }

    /// A bookmark at `spot`, named after where it is, e.g. its chapter.
    pub fn bookmark(name: String, spot: Spot) -> Self {
        Self {
            kind: MarkKind::Bookmark,
            pen: Pen::Full,
            color: None,
            word: name,
            context: String::new(),
            spot,
        }
    }

    /// The reader's highlight of `text` with `pen`.
    pub fn highlight(text: String, spot: Spot, pen: Pen, color: Option<[u8; 3]>) -> Self {
        Self {
            kind: MarkKind::Highlight,
            pen,
            color,
            word: text,
            context: String::new(),
            spot,
        }
    }
}

impl Library {
    /// New whenever any mark changes; what shows marks keys its cache on it.
    pub fn marks_stamp(&self) -> u64 {
        self.marks.stamp()
    }

    /// `book`'s living marks, oldest first.
    pub fn marks(&self, book: BookId) -> Vec<&Mark> {
        let mut out: Vec<&Mark> = self
            .marks
            .values()
            .filter(|m| m.alive.value && m.book.value == book)
            .collect();
        out.sort_by_key(|m| (m.added, m.id));
        out
    }

    /// Every book's living marks, in no order.
    pub fn every_mark(&self) -> impl Iterator<Item = &Mark> {
        self.marks.values().filter(|m| m.alive.value)
    }

    pub fn mark(&self, id: MarkId) -> Option<&Mark> {
        self.marks.get(&id).filter(|m| m.alive.value)
    }

    /// Marks the word, or finds its mark; none for an unknown book.
    pub fn add_mark(&mut self, book: BookId, new: NewMark, wall_ms: u64) -> Option<MarkId> {
        self.book(book)?;
        let same = |m: &&Mark| {
            (m.kind, m.look().pen) == (new.kind, new.pen)
                && m.word == new.word
                && m.spot.near(&new.spot)
        };
        if let Some(found) = self.marks(book).into_iter().find(same) {
            return Some(found.id);
        }
        let at = self.clock.tick(wall_ms);
        let id = self.new_mark_id(at);
        let mark = Mark {
            id,
            book: Field::new(book, at),
            kind: new.kind,
            pen: new.pen,
            color: new.color,
            word: new.word,
            context: new.context,
            spot: new.spot,
            added: wall_ms,
            alive: Field::new(true, at),
            gloss: Field::default(),
            note: Field::default(),
            style: Field::default(),
            extra: Extra::default(),
        };
        self.marks.insert(id, mark);
        Some(id)
    }

    /// Removes or brings back marks; how many changed.
    pub fn set_marks_alive(&mut self, ids: &[MarkId], alive: bool, wall_ms: u64) -> usize {
        let flips = |id: &&MarkId| {
            let mark = self.marks.get(id);
            mark.is_some_and(|m| {
                let back = alive && self.erased(m.book.value, m.alive.at);
                m.alive.value != alive && self.book(m.book.value).is_some() && !back
            })
        };
        let ids: Vec<MarkId> = ids.iter().filter(flips).copied().collect();
        if ids.is_empty() {
            return 0;
        }
        let at = self.clock.tick(wall_ms);
        for id in &ids {
            if let Some(mark) = self.marks.get_mut(id) {
                mark.alive = Field::new(alive, at);
            }
        }
        ids.len()
    }

    /// Saves or clears a living mark's answer; false if there's no such mark.
    pub fn set_gloss(&mut self, id: MarkId, gloss: Option<Gloss>, wall_ms: u64) -> bool {
        let living = self
            .mark(id)
            .is_some_and(|m| self.book(m.book.value).is_some());
        if !living {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        if let Some(mark) = self.marks.get_mut(&id) {
            mark.gloss = Field::new(gloss, at);
        }
        true
    }

    /// Writes or clears a living mark's note; blank clears it.
    pub fn set_note(&mut self, id: MarkId, note: Option<String>, wall_ms: u64) -> bool {
        let living = self
            .mark(id)
            .is_some_and(|m| self.book(m.book.value).is_some());
        if !living {
            return false;
        }
        let note = note.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
        let at = self.clock.tick(wall_ms);
        if let Some(mark) = self.marks.get_mut(&id) {
            mark.note = Field::new(note, at);
        }
        true
    }

    /// Restyles a living highlight; AI marks keep the AI's look.
    pub fn set_style(&mut self, id: MarkId, style: Style, wall_ms: u64) -> bool {
        let living = self
            .mark(id)
            .is_some_and(|m| m.kind == MarkKind::Highlight && self.book(m.book.value).is_some());
        if !living {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        if let Some(mark) = self.marks.get_mut(&id) {
            mark.style = Field::new(Some(style), at);
        }
        true
    }

    /// `gone`'s marks go to `keeper`, as a merge folding them decides.
    pub(crate) fn move_marks(&mut self, gone: BookId, keeper: BookId, at: Stamp) {
        for mark in self.marks.values_mut_where(|m| m.book.value == gone) {
            mark.book = Field::new(keeper, at);
        }
    }

    pub(crate) fn new_mark_id(&self, at: Stamp) -> MarkId {
        let state = RandomState::new();
        let seed = (at.ms, at.counter, self.device().0);
        let half = |salt: u8| u128::from(state.hash_one((seed, salt)));
        let mut id = MarkId(half(1) << 64 | half(2));
        while self.marks.contains_key(&id) {
            id = MarkId(id.0.wrapping_add(1));
        }
        id
    }
}
