//! What the player shows and is asked: the card's lines and the controls.

use std::ops::Range;

use super::parts::Kind;

/// What the player shows this frame.
pub struct View<'a> {
    pub playing: bool,
    /// The voice is there to play with.
    pub ready: bool,
    pub loading: bool,
    pub back: bool,
    pub next: bool,
    /// What a step moves by: "sentence" or "phrase".
    pub unit: &'static str,
    /// The part read in that can be left: a table, list, figure or aside.
    pub escape: Option<Kind>,
    /// The part reading holds at to ask about.
    pub ask: Option<Kind>,
    /// How much is read, 0 to 1, and seconds likely left and in all.
    pub at: Option<f32>,
    pub left: Option<f64>,
    pub total: Option<f64>,
    pub seek: bool,
    /// A fault, or the end; shown in place of the track.
    pub note: Option<(String, bool)>,
    /// Where a share of the track lands, as a page and chapter.
    pub hint: &'a dyn Fn(f32) -> Option<String>,
    /// Narrated books skip their notes.
    pub narrated: bool,
}

/// What the player was asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Back,
    Toggle,
    Next,
    Escape,
    /// The part asked about: read it, or pass it over.
    Answer(bool),
    /// To this share of the track.
    Seek(f32),
    Stop,
    /// The card, or the pill.
    Card(bool),
    /// Back to the words read, in their pane.
    Reveal,
}

/// The sentences round the one said, and which of its words are lit.
#[derive(Clone, Debug, PartialEq)]
pub struct Lyrics {
    pub before: Option<String>,
    pub now: String,
    /// Each word's bytes in `now`.
    pub words: Vec<Range<usize>>,
    /// The words lit, by index.
    pub lit: Range<usize>,
    pub preferred: Range<usize>,
    pub heard: usize,
    pub after: Option<String>,
    /// Tells one sentence from the next.
    pub key: u64,
}
