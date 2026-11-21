//! Space's small pieces: pills to pick, tags to read, captions over groups.

use egui::{Button, Color32, CornerRadius, Frame, Margin, Response, RichText, Stroke, Ui};

use crate::theme::{self, Tokens};

const ROUND: CornerRadius = CornerRadius::same(255);

/// A pill that picks or drops a choice; true when clicked.
pub(super) fn toggle(ui: &mut Ui, on: bool, text: &str, t: &Tokens) -> bool {
    let ink = if on { t.accent } else { t.ink };
    let fill = if on { t.selected } else { Color32::TRANSPARENT };
    let words = RichText::new(text).size(t.text_small).color(ink);
    let button = Button::new(words)
        .fill(fill)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(ROUND)
        .selected(on);
    ui.add(button).clicked()
}

/// A word's level, in the accent's soft tint.
pub(super) fn level(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    pill(ui, text, t.accent, t.accent_soft, t)
}

/// A quiet tag: a status, the tries, a first try.
pub(super) fn tag(ui: &mut Ui, text: &str, ink: Color32, t: &Tokens) -> Response {
    pill(ui, text, ink, t.ink.gamma_multiply(0.05), t)
}

fn pill(ui: &mut Ui, text: &str, ink: Color32, fill: Color32, t: &Tokens) -> Response {
    let words = RichText::new(text).size(t.text_small).color(ink);
    let label = egui::Label::new(words).selectable(false);
    Frame::new()
        .fill(fill)
        .corner_radius(ROUND)
        .inner_margin(Margin::symmetric(7, 1))
        .show(ui, |ui| ui.add(label))
        .inner
}

/// A small muted heading over a group of choices or rows.
pub(super) fn caption(ui: &mut Ui, text: &str, t: &Tokens) {
    let words = RichText::new(text).font(theme::medium(t.text_small));
    ui.add(egui::Label::new(words.color(t.muted)).selectable(false));
}

/// Muted words, small.
pub(super) fn quiet(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    let words = RichText::new(text).size(t.text_small).color(t.muted);
    ui.add(egui::Label::new(words).selectable(false))
}
