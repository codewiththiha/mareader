//! Reads a narrated book aloud: its player, the phrase lit, the bar.

use std::ops::Range;
use std::sync::Arc;

use egui::{Rect, Ui};
use mareader_engine::command::Command;
use mareader_engine::read_aloud::booth::Booth;
use mareader_engine::read_aloud::lit::Lit;
use mareader_engine::read_aloud::parts;
use mareader_engine::read_aloud::place::Stand;
use mareader_engine::read_aloud::state::{Origin, State as ReadAloudState};
use mareader_session::Anchor;
use mareader_settings::Player as Look;
use mareader_workspace::PaneId;

use crate::documents::Reading;
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

mod ask;
mod bar;
mod bar_layout;
mod card;
mod control;
mod edges;
mod highlight;
pub mod marker;
mod marker_paint;
mod orb;
mod tab;

pub use highlight::paint;

/// Seconds the player takes to rise into place.
const APPEAR: f64 = 0.22;
/// Seconds the pill takes to open into the card, and back.
const MORPH: f32 = 0.34;
/// Seconds the words lit may be out of sight before the card shows.
const HIDDEN: f64 = 0.8;
/// The player's place closes this share of its gap each second, roughly.
const GLIDE: f32 = 14.0;
/// The speaker on a word's card, and the same button while it speaks.
pub const SAY: &str = "Say aloud";
pub const STOP_SAYING: &str = "Stop saying";

/// Where the player may show, and whether the words read are in sight.
pub struct Stage<'a> {
    /// The panes are up: the reader is the page shown.
    pub reader: bool,
    pub panes: &'a [(PaneId, Rect)],
    /// The pane in front: its room keeps the player once its book is closed.
    pub focused: Option<PaneId>,
    /// The window's room below the title bar.
    pub content: Rect,
    /// Whether the words lit are in sight, when they can be told.
    pub shown: Option<bool>,
    /// The title of the book read.
    pub title: &'a str,
    /// Library identity for per-book choices; none for an unsaved document.
    pub book: Option<mareader_library::BookId>,
    /// Its pane's own look, while it has one; else the window's.
    pub tint: Option<Tokens>,
    /// Active toasts occupying the corner, if any.
    pub toasts: Option<Rect>,
    /// The Dictionary's floating window, if it is up.
    pub dict: Option<Rect>,
    pub parking: mareader_settings::EdgeParking,
}

/// The player's colours, easing from one look to the next.
#[derive(Clone, Copy)]
struct Tint {
    from: Tokens,
    to: Tokens,
    since: f64,
}

impl Tint {
    /// The colours at `now`, eased over `time` seconds.
    fn at(&self, now: f64, time: f64) -> Tokens {
        let k = match time > 0.0 {
            true => ((now - self.since) / time).clamp(0.0, 1.0) as f32,
            false => 1.0,
        };
        self.from.toward(&self.to, ease_in_out(k))
    }
}

/// The player's shape and place, kept between frames.
#[derive(Default)]
struct Shape {
    /// The card or the pill, asked by hand until the need for it changes.
    chosen: Option<bool>,
    /// The words read can't be seen, so the card would help.
    auto: bool,
    /// Since when the words lit have been out of sight.
    hidden_since: Option<f64>,
    /// Distinguishes a scroll-away from a highlight hidden at first.
    highlight_seen: bool,
    /// Whether the card is wanted, this frame.
    card: bool,
    /// The foot centre it sits on, gliding where it should.
    at: Option<egui::Pos2>,
    /// The panel's current rect, shared with dictionary avoidance.
    rect: Option<Rect>,
    /// The player sliding partly beyond the reader's bottom edge.
    parked: Option<crate::park::Tuck>,
    orb_target: Option<Rect>,
    /// A short grace period while the player is summoned or restored.
    unparked_until: Option<f64>,
    /// A reveal or shape change keeps the dictionary from covering the player.
    player_priority: bool,
    /// The mode last shown, to detect a user change.
    last_look: Option<Look>,
    time: Option<f64>,
    scroll: card::Scroll,
    ticker: card::Ticker,
    /// When the player first showed for this reading.
    appeared: Option<f64>,
    /// Its width while a side is dragged.
    dragged: Option<f32>,
    /// The width it eases to while toasts hold the corner.
    dodge_toast: Option<f32>,
    tint: Option<Tint>,
    /// How far it stands above its book's pane foot; 0 elsewhere.
    rise: f32,
    /// The time left it shows, running down evenly.
    countdown: mareader_engine::read_aloud::countdown::Countdown,
    /// The part last asked about, kept while its question fades.
    asked: Option<parts::Kind>,
    /// Opted in for the current unanswered question only.
    remember_skip: bool,
}

/// Window-ephemeral read-aloud UI state: player shape, animations, parking.
#[derive(Default)]
pub struct ReadAloudUi {
    shape: Shape,
}

/// Read-only view over engine read-aloud state and window player overlays.
#[derive(Clone, Copy)]
pub struct ReadAloudView<'a> {
    pub booth: &'a Booth,
    pub shelf: &'a mareader_speech::Shelf,
    state: &'a ReadAloudState,
    ui: &'a ReadAloudUi,
}

impl<'a> ReadAloudView<'a> {
    pub fn new(state: &'a ReadAloudState, ui: &'a ReadAloudUi) -> Self {
        Self {
            booth: &state.booth,
            shelf: &state.shelf,
            state,
            ui,
        }
    }

    pub fn active(&self) -> bool {
        self.state.active()
    }

    pub fn playing(&self) -> bool {
        self.state.playing()
    }

    pub fn is_asking(&self) -> bool {
        self.state.is_asking()
    }

    pub fn away(&self) -> Option<&'a Origin> {
        self.state.away()
    }

    pub fn origin(&self) -> Option<&'a Origin> {
        self.state.origin()
    }

    pub fn reading_spot(&self) -> Option<(usize, Option<Range<usize>>)> {
        self.state.reading_spot()
    }

    pub fn home(&self) -> Option<PaneId> {
        self.state.home()
    }

    pub fn saying(&self) -> Option<&'a str> {
        self.state.saying()
    }

    pub fn words_said(&self) -> u64 {
        self.state.words_said()
    }

    pub fn said(&self) -> Option<(PaneId, Anchor)> {
        self.state.said()
    }

    pub fn lit(&self, pane: PaneId) -> Option<Lit> {
        self.state.lit(pane)
    }

    pub fn spot(&self) -> Option<(PaneId, usize, Option<Range<usize>>)> {
        self.state.spot()
    }

    pub fn colours(&self) -> Option<Tokens> {
        self.ui.colours()
    }

    pub fn carded(&self) -> bool {
        self.state.active() && self.ui.shape.card
    }

    pub fn island_anchor(&self) -> Option<Rect> {
        self.ui.island_anchor(self.state.active())
    }

    pub fn clearance_rect(&self, now: f64) -> Option<Rect> {
        self.ui.clearance_rect(self.state.active(), now)
    }
}

impl ReadAloudUi {
    /// Updates the player bar's transient state after playback starts.
    pub fn on_started(&mut self, fresh: bool) {
        if fresh {
            self.shape = Shape::default();
        } else {
            self.shape.hidden_since = None;
            self.shape.highlight_seen = false;
        }
    }

    /// Takes any remembered skip part from the ask dialog and resets it.
    pub fn take_answer_remember(&mut self) -> Option<parts::Kind> {
        let remember = self
            .shape
            .remember_skip
            .then_some(self.shape.asked)
            .flatten();
        self.shape.remember_skip = false;
        remember
    }

    pub fn rise(&self) -> f32 {
        self.shape.rise
    }

    /// The look the player wears, or eases to, once it has shown.
    pub fn colours(&self) -> Option<Tokens> {
        self.shape.tint.map(|t| t.to)
    }

    /// Lets a dictionary interaction reclaim its space.
    pub fn yield_to_dictionary(&mut self) {
        self.shape.player_priority = false;
        self.shape.unparked_until = None;
    }

    pub fn island_anchor(&self, active: bool) -> Option<Rect> {
        if !active
            || self.shape.parked.is_some()
            || self.shape.orb_target.is_some()
            || self.shape.card
        {
            return None;
        }
        self.shape.rect
    }

    /// The player's intended rect while summoned, restoring or kept open.
    pub fn clearance_rect(&self, active: bool, now: f64) -> Option<Rect> {
        if !active {
            return None;
        }
        let clear = self.shape.player_priority
            || self.shape.unparked_until.is_some_and(|until| now < until);
        match self.shape.parked {
            Some(tuck) if tuck.revealing || clear => Some(tuck.home),
            Some(_) => None,
            None if clear => self.shape.rect,
            None => None,
        }
    }
}

/// Slow, quick, then slow: for things changing shape.
fn ease_in_out(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// Fast at first, then settling: for things arriving.
fn ease_out(k: f64) -> f64 {
    1.0 - (1.0 - k).powi(3)
}

/// The speaker on a word's card: says it, or stops it; accented meanwhile.
pub fn say_button(
    ui: &mut Ui,
    word: &str,
    (saying, voice): (bool, Option<&str>),
    out: &mut Commands,
) {
    // No voice here speaks Burmese and its kin.
    if crate::shaped::needs_shaping(word) {
        return;
    }
    let (label, command) = match saying {
        true => (STOP_SAYING, Sent::Engine(Command::StopSaying)),
        false => (SAY, say(word, voice)),
    };
    let accent = Tokens::get(ui.ctx()).accent;
    let r = ui.scope(|ui| {
        if saying {
            ui.visuals_mut().override_text_color = Some(accent);
        }
        widgets::small_icon_button(ui, Icon::Speak, label)
    });
    if r.inner.clicked() {
        out.push(command);
    }
}

/// Says `word`, in `voice` when it isn't the one picked.
pub fn say(word: &str, voice: Option<&str>) -> Sent {
    let word = word.to_owned();
    match voice {
        Some(voice) => Sent::Engine(Command::SayWordWith {
            word,
            voice: voice.to_owned(),
        }),
        None => Sent::Engine(Command::SayWord(word)),
    }
}

pub(crate) fn rate_label(rate: f32) -> String {
    let text = format!("{rate:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}×")
}

/// A pane's book, as the reader below the front end sees it.
impl Reading {
    /// Where it stands: its pane, its book, and the page showing.
    pub fn stand(&self) -> Stand {
        Stand {
            pane: self.pane,
            doc: self.doc,
            document: Arc::clone(&self.document),
            current: self.current,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_settings::ReadAloudSettings;

    #[test]
    fn settings_and_speech_name_the_system_voice_alike() {
        // Settings can't see speech, so each spells the id itself.
        assert_eq!(ReadAloudSettings::SYSTEM, mareader_speech::SYSTEM);
    }

    #[test]
    fn speeds_read_short() {
        assert_eq!(rate_label(1.0), "1×");
        assert_eq!(rate_label(1.25), "1.25×");
        assert_eq!(rate_label(0.75), "0.75×");
        assert_eq!(rate_label(1.5), "1.5×");
    }
}
