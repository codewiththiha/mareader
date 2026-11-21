//! Study mode in the reader: the page menu's Study section, the veil over
//! locked pages and the deck at the gate.
//!
//! The engine holds every run (`mareader_engine::study_mode`); this module
//! only draws a run and sends what the reader asks. A pane's view stops at
//! its gate in `pdf_view`; here the pages past it are veiled, and the deck
//! covers the pane once the gate is in view. Until then a pill tells how
//! the next deck is coming along. In Reactive mode a click on a shaded word
//! opens a recall beside it instead.

pub mod deck;
pub mod gate;
pub mod menu;
pub mod pill;
pub mod recall;
pub mod waiting;

use std::collections::HashMap;
use std::ops::Range;

use egui::{Context, Rect};
use mareader_engine::pane::ViewMode;
use mareader_engine::session::Session;
use mareader_engine::study_mode::{DeckState, Run};
use mareader_settings::{BarItem, HighlightSettings};
use mareader_study::mode::{Mode, Outcome};
use mareader_workspace::PaneId;

use crate::selection::Sheet;
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    /// The batch it follows, by its first and last page.
    batch: (usize, usize),
    pub at: usize,
    pub revealed: bool,
}

impl Cursor {
    /// Moves `by` cards, staying within `count`; the next card starts hidden.
    pub fn step(&mut self, by: isize, count: usize) {
        let last = count.saturating_sub(1);
        self.at = self.at.saturating_add_signed(by).min(last);
        self.revealed = false;
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
    /// Start waits for the time floor.
    pub strict: bool,
    pub now: f64,
    /// The page the pane reads.
    pub current: usize,
    /// A dictionary pack can write the cards instead.
    pub dict: bool,
}

/// The window's half of study mode: each deck's cursor and the keys' pane.
#[derive(Default)]
pub struct StudyModeUi {
    cursors: HashMap<PaneId, Cursor>,
    /// The focused pane whose deck showed last frame; it takes the keys.
    keyed: Option<PaneId>,
    /// Shots: the shown card turns over once the deck opens.
    pub reveal_shot: bool,
    /// The answer typed in the open recall.
    pub recall: recall::Draft,
    /// Shots: the recall to open, as `word` or `word:answer`.
    pub recall_shot: Option<String>,
}

impl StudyModeUi {
    /// Keeps the decks of the panes in `studying` and forgets the rest.
    pub fn begin(&mut self, studying: &[PaneId]) {
        self.cursors.retain(|pane, _| studying.contains(pane));
        self.keyed = None;
    }

    /// The veil past `pane`'s gate, or its deck once the gate is in view.
    pub fn show(&mut self, ctx: &Context, pane: &Pane<'_>, out: &mut Commands) {
        let Some(batch) = pane.run.gate_batch() else {
            return;
        };
        let Some(gate) = pane.run.gate() else {
            return;
        };
        let veil = gate::veil(pane.sheets, pane.rect, gate, pane.mode);
        if veil.is_none_or(|v| !v.reached) {
            if let Some(veil) = veil {
                let range = batch.label();
                gate::cover(ctx, pane.id, veil.rect, |ui| {
                    gate::prompt(ui, pane.id, &range, out);
                });
            }
            pill::show(ctx, pane, veil.map(|v| v.rect.top()), out);
            return;
        }
        let cursor = self.cursors.entry(pane.id).or_default();
        if cursor.batch != (batch.from, batch.to) {
            *cursor = Cursor {
                batch: (batch.from, batch.to),
                ..Cursor::default()
            };
        }
        if self.reveal_shot && matches!(pane.run.deck, DeckState::Open(_)) {
            self.reveal_shot = false;
            cursor.revealed = true;
        }
        gate::cover(ctx, pane.id, pane.rect, |ui| {
            deck::show(ui, pane, cursor, out);
        });
        if pane.active {
            self.keyed = Some(pane.id);
        }
    }

    /// The deck's keys, taken before the reader's own keys see them.
    pub fn keys(&mut self, ctx: &Context, session: &Session, out: &mut Commands) {
        let Some(pane) = self.keyed else {
            return;
        };
        if ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) {
            return;
        }
        let Some(run) = session.study_mode().run(pane) else {
            return;
        };
        let DeckState::Open(open) = &run.deck else {
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
    fn a_cursor_stays_on_the_deck_and_hides_each_new_card() {
        let mut cursor = Cursor {
            revealed: true,
            ..Cursor::default()
        };
        cursor.step(-1, 3);
        assert_eq!((cursor.at, cursor.revealed), (0, false));
        cursor.step(5, 3);
        assert_eq!(cursor.at, 2);
        cursor.step(1, 0);
        assert_eq!(cursor.at, 0);
    }
}
