//! Study mode in the reader: the page menu's Study section, the veil over
//! locked pages and the deck at the gate.
//!
//! The engine holds every run (`mareader_engine::study_mode`); this module
//! only draws a run and sends what the reader asks. A pane's view stops at
//! its gate in `pdf_view`; here the pages past it are veiled, and the deck
//! covers the pane once the gate is in view.

pub mod deck;
pub mod gate;
pub mod menu;

use std::collections::HashMap;

use egui::{Context, Rect};
use mareader_engine::pane::ViewMode;
use mareader_engine::session::Session;
use mareader_engine::study_mode::{DeckState, Run};
use mareader_settings::{BarItem, HighlightSettings};
use mareader_workspace::PaneId;

use crate::selection::Sheet;
use crate::ui_command::Commands;

/// The page's shading while `pane` studies: kept only with hints on.
pub fn shading(
    session: &Session,
    pane: PaneId,
    look: Option<crate::vocab::Look>,
) -> Option<crate::vocab::Look> {
    let hints = session.settings().study.mode.hints;
    let hidden = session.study_mode().on(pane) && !hints;
    look.filter(|_| !hidden)
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
}

/// The window's half of study mode: each deck's cursor and the keys' pane.
#[derive(Default)]
pub struct StudyModeUi {
    cursors: HashMap<PaneId, Cursor>,
    /// The focused pane whose deck showed last frame; it takes the keys.
    keyed: Option<PaneId>,
    /// Shots: the shown card turns over once the deck opens.
    pub reveal_shot: bool,
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
        let Some(veil) = gate::veil(pane.sheets, pane.rect, gate, pane.mode) else {
            return;
        };
        if !veil.reached {
            let range = batch.label();
            gate::cover(ctx, pane.id, veil.rect, |ui| {
                gate::prompt(ui, pane.id, &range, out);
            });
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
        assert!(look.bar.contains(&BarItem::Explain), "the default bar explains");
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
