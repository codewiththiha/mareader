//! Study mode in the reader: the page menu's Study section, the veil over
//! locked pages and the deck at the gate.
//!
//! The engine holds every run (`mareader_engine::study_mode`); this module
//! only draws a run and sends what the reader asks. A pane's view stops at
//! its gate in `pdf_view`; here the pages past it are veiled, and the deck
//! covers the pane once the gate is in view, unless the reader left it for
//! the pages before. A corner toast tells how the AI is getting on with the
//! cards. In Reactive mode a click on a shaded word opens a recall beside it
//! instead.

pub mod confirm;
pub mod deck;
pub mod flip_card;
pub mod gate;
pub mod menu;
pub mod progress;
pub mod recall;
pub mod waiting;

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use egui::{Context, Rect};
use mareader_engine::pane::ViewMode;
use mareader_engine::session::Session;
use mareader_engine::study_mode::{Action, DeckState, Run};
use mareader_settings::{BarItem, HighlightSettings};
use mareader_study::mode::{Batch, Mode, Outcome};
use mareader_workspace::PaneId;

use crate::selection::Sheet;
use crate::thumbnails::StudyMarks;
use crate::ui_command::Commands;
use crate::vocab::Settled;

/// The page's shading while `pane` studies: kept only with hints on.
///
/// Reactive mode keeps it, a click recalling the word and a rest doing nothing.
pub fn shading(
    session: &Session,
    pane: PaneId,
    look: Option<crate::vocab::Look>,
) -> Option<crate::vocab::Look> {
    let run = session.study_mode().run(pane);
    if run.is_some_and(|r| r.mode == Mode::Reactive) {
        return look.map(|look| crate::vocab::Look {
            click: true,
            hover: false,
            ..look
        });
    }
    let hints = session.settings().study.mode.hints;
    let hidden = run.is_some() && !hints;
    look.filter(|_| !hidden)
}

/// What the page grid marks of `pane`'s Proactive study.
pub fn marks(session: &Session, pane: PaneId) -> StudyMarks {
    let run = session.study_mode().run(pane);
    let Some(run) = run.filter(|r| r.mode == Mode::Proactive) else {
        return StudyMarks::default();
    };
    let place = session.documents().place(pane);
    let current = place.map_or(run.start, |place| place.current);
    let batch = run.batches.iter().find(|b| b.holds(current));
    StudyMarks {
        on: true,
        batch: batch.map(|b| (b.from, b.to)),
        start: run.start,
        lock: session.settings().study.mode.lock,
        gate: run.gate(),
    }
}

/// How far study mode lets `pane` go: its locked start up to its gate.
pub fn span(session: &Session, pane: PaneId) -> gate::StudySpan {
    gate::StudySpan {
        floor: session.study_floor(pane),
        gate: session.study_gate(pane),
    }
}

/// The words `pane` recalled this run, in the solved or the missed look.
pub fn settled(session: &Session, pane: PaneId) -> Vec<Settled> {
    let looks = &session.settings().study.highlights;
    let Some(run) = session.study_mode().run(pane) else {
        return Vec::new();
    };
    let ink = |outcome: &Outcome| match outcome {
        Outcome::Missed => looks.missed,
        Outcome::Learning | Outcome::Memorized => looks.solved,
    };
    let settle = |(page, chars, outcome): &(usize, Range<usize>, Outcome)| Settled {
        page: *page,
        start: chars.start,
        ink: ink(outcome),
    };
    run.recalled.iter().map(settle).collect()
}

/// The selection bar while studying: no Explain, no Look up.
pub fn bar(look: &HighlightSettings, studying: bool) -> HighlightSettings {
    let mut look = look.clone();
    if studying {
        let help = |item: &BarItem| matches!(item, BarItem::Explain | BarItem::LookUp);
        look.bar.retain(|item| !help(item));
    }
    look
}

/// One pane's place in its deck: the card shown, and whether it is turned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    /// The batch it follows, by its first and last page.
    batch: (usize, usize),
    pub at: usize,
    pub turned: bool,
    /// The cards in the order shown, by their place in the deck.
    pub order: Vec<usize>,
}

impl Cursor {
    /// Moves `by` cards, staying within `count`; the next card starts face up.
    pub fn step(&mut self, by: isize, count: usize) {
        let last = count.saturating_sub(1);
        self.at = self.at.saturating_add_signed(by).min(last);
        self.turned = false;
    }

    /// Keeps the order to a deck of `count`, in its own order if it changed.
    pub fn fit(&mut self, count: usize) {
        if self.order.len() != count {
            self.order = (0..count).collect();
        }
        self.at = self.at.min(count.saturating_sub(1));
    }

    /// The deck's place of the card shown.
    pub fn card(&self) -> Option<usize> {
        self.order.get(self.at).copied()
    }

    /// Forgets the card at deck place `card`, as the engine drops it.
    pub fn drop_card(&mut self, card: usize) {
        self.order.retain(|&i| i != card);
        for i in &mut self.order {
            if *i > card {
                *i -= 1;
            }
        }
        self.turned = false;
    }

    /// Shows the cards in a new order, from the first.
    pub fn shuffle(&mut self, seed: u64) {
        let mut state = seed | 1;
        for i in (1..self.order.len()).rev() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let j = (state % (i as u64 + 1)) as usize;
            self.order.swap(i, j);
        }
        self.at = 0;
        self.turned = false;
    }
}

/// What a pane's veil and deck are drawn from, this frame.
pub struct Pane<'a> {
    pub id: PaneId,
    pub rect: Rect,
    pub run: &'a Run,
    pub sheets: &'a [Sheet],
    pub mode: ViewMode,
    pub active: bool,
    /// The pages before the run's start are locked.
    pub lock: bool,
    pub now: f64,
    /// The page the pane reads.
    pub current: usize,
    /// A dictionary pack can write the cards instead.
    pub dict: bool,
    /// The pane's page bar is up; corner marks rise over it.
    pub bar: bool,
}

/// The window's half of study mode: each deck's cursor and the keys' pane.
#[derive(Default)]
pub struct StudyModeUi {
    cursors: HashMap<PaneId, Cursor>,
    /// Each pane's corner toast, across frames.
    watches: HashMap<PaneId, progress::Watch>,
    /// The panes whose open deck showed last frame.
    showing: HashSet<PaneId>,
    /// The question a deck asks before it acts, and its pane.
    asking: Option<(PaneId, confirm::Ask)>,
    /// The focused pane whose deck showed last frame; it takes the keys.
    keyed: Option<PaneId>,
    /// Shots: the shown card turns over once the deck opens.
    pub reveal_shot: bool,
    /// Shots: the shown card held this far aside, as if dragged.
    pub drag_shot: Option<f32>,
    /// The answer typed in the open recall.
    pub recall: recall::Draft,
    /// Shots: the recall to open, as `word` or `word:answer`.
    pub recall_shot: Option<String>,
}

impl StudyModeUi {
    /// Keeps the decks of the panes in `studying` and forgets the rest.
    pub fn begin(&mut self, studying: &[PaneId]) {
        self.cursors.retain(|pane, _| studying.contains(pane));
        self.watches.retain(|pane, _| studying.contains(pane));
        self.showing.retain(|pane| studying.contains(pane));
        let gone = |(p, _): &(PaneId, confirm::Ask)| !studying.contains(p);
        if self.asking.as_ref().is_some_and(gone) {
            self.asking = None;
        }
        self.keyed = None;
    }

    /// The veil past `pane`'s gate, or its deck once the gate is in view.
    pub fn show(&mut self, ctx: &Context, pane: &Pane<'_>, out: &mut Commands) {
        let now = ctx.input(|i| i.time);
        let watch = self.watches.entry(pane.id).or_default();
        if let Some(work) = watch.status(progress::work(pane.run), now) {
            progress::show(ctx, pane, work, watch.alpha(now));
        }
        let shown = self.deck_or_veil(ctx, pane, out);
        let floor = pane.run.floor(pane.lock);
        let frost = gate::frost(pane.sheets, pane.rect, floor, pane.mode);
        if let Some(rect) = frost.filter(|_| !shown) {
            gate::before(ctx, pane.id, rect);
        }
        if self.showing.contains(&pane.id) != shown {
            match shown {
                true => self.showing.insert(pane.id),
                false => self.showing.remove(&pane.id),
            };
            let pane = pane.id;
            deck::send(out, Action::Shown { pane, shown });
        }
        let keep = match &self.asking {
            Some((id, ask)) if *id == pane.id => confirm::show(ctx, *id, ask, out),
            _ => true,
        };
        if !keep {
            self.asking = None;
        }
    }

    /// A deck opened again, the gate's deck, or the veil before it; true
    /// while an open deck shows.
    fn deck_or_veil(&mut self, ctx: &Context, pane: &Pane<'_>, out: &mut Commands) -> bool {
        if let Some(review) = &pane.run.review {
            return self.cover(ctx, pane, &review.batch, out);
        }
        let Some(batch) = pane.run.gate_batch() else {
            return false;
        };
        let Some(gate) = pane.run.gate() else {
            return false;
        };
        let veil = gate::veil(pane.sheets, pane.rect, gate, pane.mode);
        let away = matches!(&pane.run.deck, DeckState::Open(open) if open.away);
        if away || veil.is_none_or(|v| !v.reached) {
            if let Some(veil) = veil {
                let range = batch.label();
                gate::cover(ctx, pane.id, veil.rect, |ui| {
                    gate::prompt(ui, pane.id, (&range, away), out);
                });
            }
            return false;
        }
        self.cover(ctx, pane, batch, out)
    }

    /// The deck for `batch` over the whole pane; true while it is open.
    fn cover(&mut self, ctx: &Context, pane: &Pane<'_>, batch: &Batch, out: &mut Commands) -> bool {
        let cursor = self.cursors.entry(pane.id).or_default();
        if cursor.batch != (batch.from, batch.to) {
            *cursor = Cursor {
                batch: (batch.from, batch.to),
                ..Cursor::default()
            };
        }
        let open = pane.run.shown_deck().is_some();
        if self.reveal_shot && open {
            self.reveal_shot = false;
            cursor.turned = true;
        }
        if let Some(dx) = self.drag_shot {
            flip_card::hold(ctx, deck::card_id(pane.id, cursor.batch), dx);
        }
        let mut ask = None;
        gate::cover(ctx, pane.id, pane.rect, |ui| {
            deck::show(ui, pane, cursor, &mut ask, out);
        });
        if let Some(ask) = ask {
            self.asking = Some((pane.id, ask));
        }
        if pane.active {
            self.keyed = Some(pane.id);
        }
        open
    }

    /// The deck's keys, taken before the reader's own keys see them.
    pub fn keys(&mut self, ctx: &Context, session: &Session, out: &mut Commands) {
        let Some(pane) = self.keyed else {
            return;
        };
        let asking = self.asking.is_some();
        if asking || ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) {
            return;
        }
        let Some(run) = session.study_mode().run(pane) else {
            return;
        };
        let Some(open) = run.shown_deck() else {
            return;
        };
        let Some(cursor) = self.cursors.get_mut(&pane) else {
            return;
        };
        if let Some(press) = ctx.input_mut(deck::Press::read) {
            deck::press(cursor, open, pane, press, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studying_leaves_the_word_help_off_the_selection_bar() {
        let look = HighlightSettings::default();
        assert!(
            look.bar.contains(&BarItem::Explain),
            "the default bar explains"
        );
        let kept = bar(&look, true);
        assert!(!kept.bar.contains(&BarItem::Explain));
        assert!(!kept.bar.contains(&BarItem::LookUp));
        assert_eq!(bar(&look, false), look);
    }

    #[test]
    fn a_cursor_stays_on_the_deck_and_shows_each_new_card_face_up() {
        let mut cursor = Cursor {
            turned: true,
            ..Cursor::default()
        };
        cursor.step(-1, 3);
        assert_eq!((cursor.at, cursor.turned), (0, false));
        cursor.step(5, 3);
        assert_eq!(cursor.at, 2);
        cursor.step(1, 0);
        assert_eq!(cursor.at, 0);
    }

    #[test]
    fn a_shuffle_keeps_every_card_and_a_known_one_leaves() {
        let mut cursor = Cursor::default();
        cursor.fit(5);
        cursor.at = 3;
        cursor.shuffle(42);
        let mut sorted = cursor.order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, [0, 1, 2, 3, 4]);
        assert_eq!(cursor.at, 0);
        let first = cursor.card();
        cursor.drop_card(2);
        assert_eq!(cursor.order.len(), 4);
        assert!(cursor.order.iter().all(|&i| i < 4));
        if first != Some(2) {
            assert_eq!(cursor.card(), first.map(|c| if c > 2 { c - 1 } else { c }));
        }
    }
}
