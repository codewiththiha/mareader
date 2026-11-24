//! The deck's rows under the card: the steps and the known button. A
//! compact deck puts the steps on the card's edges and the marks in its
//! footer.

use egui::{Align2, Rect, Ui, Vec2};
use mareader_engine::study_mode::{Action, Open};

use super::controls::{self, Chip, Look};
use super::deck::{self, KNOWN};
use super::{Cursor, Pane};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::Commands;
use crate::widgets;

const PREVIOUS: &str = "Previous card";
const NEXT: &str = "Next card";
const UNDO: &str = "Undo known";
/// The room the counter takes between the step buttons.
const STEP_GAP: f32 = 96.0;
/// The round step buttons on a compact card's edges.
const EDGE: f32 = 30.0;
/// What knowing a word does.
const SKIPPED: &str = "Skipped in every book from now on";

/// Back and forward between cards, with where this one sits between.
pub(super) fn steps(ui: &mut Ui, cursor: &mut Cursor, count: usize) {
    let pressed = controls::row(ui, step_chips(cursor, count).into(), STEP_GAP);
    if pressed.first().is_some_and(egui::Response::clicked) {
        cursor.step(-1, count);
    }
    if pressed.get(1).is_some_and(egui::Response::clicked) {
        cursor.step(1, count);
    }
    counter(ui, &pressed, cursor, count);
}

/// Back and forward, each off at its end of the deck.
fn step_chips(cursor: &Cursor, count: usize) -> [Chip<'static>; 2] {
    let first = cursor.at == 0;
    let last = cursor.at.saturating_add(1) >= count;
    let back = Chip::new(PREVIOUS, Look::Soft)
        .bare(Icon::ChevronLeft)
        .key("←")
        .enabled(!first);
    let next = Chip::new(NEXT, Look::Soft)
        .bare(Icon::ChevronRight)
        .key("→")
        .enabled(!last);
    [back, next]
}

/// Which card this is, between the first two of `pressed`.
fn counter(ui: &Ui, pressed: &[egui::Response], cursor: &Cursor, count: usize) {
    let t = Tokens::get(ui.ctx());
    let (Some(back), Some(next)) = (pressed.first(), pressed.get(1)) else {
        return;
    };
    let said = format!("{} / {count}", cursor.at.saturating_add(1));
    let x = (back.rect.right() + next.rect.left()) / 2.0;
    let at = egui::pos2(x, back.rect.center().y);
    let font = theme::medium(14.0);
    let painter = ui.painter();
    painter.text(at, Align2::CENTER_CENTER, &said, font, t.muted);
}

/// Know this word, take that back.
pub(super) fn marks(
    ui: &mut Ui,
    pane: &Pane<'_>,
    open: &Open,
    cursor: &mut Cursor,
    out: &mut Commands,
) {
    let shown = cursor.card().and_then(|i| open.cards.get(i));
    let undo = open.can_undo();
    let mut chips = Vec::new();
    if shown.is_some() {
        chips.push(Chip::new(KNOWN, Look::Outline).icon(Icon::Check).key("K"));
    }
    if undo {
        chips.push(Chip::new(UNDO, Look::Outline).bare(Icon::Restore));
    }
    if chips.is_empty() {
        return;
    }
    let pressed = controls::row(ui, chips, 8.0);
    let known_at = shown.is_some().then_some(0);
    if let Some(mark) = known_at.and_then(|i| pressed.get(i)) {
        widgets::tip(mark, SKIPPED, Some("K"));
    }
    if let (Some(shown), true) = (shown, pressed.first().is_some_and(egui::Response::clicked)) {
        deck::known(pane.id, shown, cursor, out);
    } else if undo && pressed.last().is_some_and(egui::Response::clicked) {
        deck::send(out, Action::Undo(pane.id));
    }
}

/// Back and forward as round buttons on the card's edges, while they step.
pub(super) fn edges(ui: &mut Ui, card: Rect, cursor: &mut Cursor, count: usize) {
    let t = Tokens::get(ui.ctx());
    let last = cursor.at.saturating_add(1) >= count;
    let ends = [
        (card.left(), -1, Icon::ChevronLeft, PREVIOUS, cursor.at > 0),
        (card.right(), 1, Icon::ChevronRight, NEXT, !last),
    ];
    for (x, by, icon, label, open) in ends {
        let rect = Rect::from_center_size(egui::pos2(x, card.center().y), Vec2::splat(EDGE));
        if open && controls::round(ui, rect, icon, label, &t).clicked() {
            cursor.step(by, count);
        }
    }
}

/// Know this word and take that back, bare, to lead the compact footer.
pub(super) fn lead(open: &Open, cursor: &Cursor) -> Vec<Chip<'static>> {
    let mut chips = Vec::new();
    if cursor.card().is_some_and(|i| i < open.cards.len()) {
        chips.push(Chip::new(KNOWN, Look::Outline).key("K").bare(Icon::Check));
    }
    if open.can_undo() {
        chips.push(Chip::new(UNDO, Look::Outline).bare(Icon::Restore));
    }
    chips
}

/// Acts on the chips [`lead`] laid out, `pressed` in its order.
pub(super) fn led(
    pressed: &[egui::Response],
    pane: &Pane<'_>,
    open: &Open,
    cursor: &mut Cursor,
    out: &mut Commands,
) {
    let clicked = |i: usize| pressed.get(i).is_some_and(egui::Response::clicked);
    let shown = cursor.card().and_then(|i| open.cards.get(i));
    if let Some(mark) = pressed.first().filter(|_| shown.is_some()) {
        widgets::tip(mark, SKIPPED, Some("K"));
    }
    let undo = usize::from(shown.is_some());
    if let (Some(shown), true) = (shown, clicked(0)) {
        deck::known(pane.id, shown, cursor, out);
    } else if open.can_undo() && clicked(undo) {
        deck::send(out, Action::Undo(pane.id));
    }
}
