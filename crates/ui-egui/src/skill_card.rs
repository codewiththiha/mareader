//! A skill's answer, in a card pointing at the text it was asked about.
//!
//! The card is plain: the skill's name, the answer, who gave it. It rides
//! its page as AI cards do and hides while its place is scrolled away.
//! Its width follows the text. See `docs/selection-ai.md`.

use egui::{Align, Context, Id, Layout, Margin, Rect, RichText, ScrollArea, Ui, vec2};
use mareader_engine::command::Command;
use mareader_engine::skills::{Action, Anchor, Answer, Ask};
use mareader_workspace::PaneId;

use crate::bubble::{Bubble, Side};
use crate::documents::DocumentsView;
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::{motion, widgets, word_card};

mod indicators;

/// Narrowest and widest a card grows, before the pane's own width.
pub const LEAST: f32 = 360.0;
pub const MOST: f32 = 560.0;
/// Width added per square root of a character, past the first 400.
const GROWTH: f32 = 5.0;
/// The answer's most height before it scrolls.
const BODY_MOST: f32 = 460.0;
const BODY_LEAST: f32 = 72.0;
/// The answer's type size: a touch over the word card's, as it reads long.
const TEXT_GROW: f32 = 1.0;

/// The card's width for `chars` of text: grows with the square root, so
/// doubling the text widens it by less, then stops at [`MOST`].
pub fn width_for(chars: usize) -> f32 {
    let grown = LEAST - 2.0 * GROWTH * 10.0 + (chars as f32).sqrt() * GROWTH;
    grown.clamp(LEAST, MOST)
}

/// Where `ask`'s card points, when that is on screen in its pane.
pub fn in_view(
    ask: &Ask,
    panes: &[(PaneId, Rect)],
    docs: DocumentsView<'_>,
) -> Option<(Rect, Rect)> {
    if docs
        .reading(ask.pane)
        .is_none_or(|r| r.doc.get() != ask.doc)
    {
        return None;
    }
    let room = panes
        .iter()
        .find(|(p, _)| *p == ask.pane)
        .map(|(_, r)| *r)?;
    let at = anchor_rect(ask, docs).filter(|r| crate::sight::visible(room, *r))?;
    Some((at, room))
}

/// Draws every ask's card whose place is on screen in its pane.
pub fn show(
    ctx: &Context,
    asks: &[Ask],
    (panes, docs): (&[(PaneId, Rect)], DocumentsView<'_>),
    clearance: (f32, f32),
    out: &mut Commands,
) {
    let t = Tokens::get(ctx);
    crate::skill_outline::show(ctx, asks, panes, docs);
    let mut indicators = indicators::Indicators::new(clearance);
    for ask in asks {
        let Some((at, room)) = in_view(ask, panes, docs) else {
            continue;
        };
        if !ask.open {
            if ask.answer.landed() && matches!(ask.anchor, Anchor::Text(_)) {
                indicators.push(ask, at, room);
            }
            continue;
        }
        let key = mareader_engine::activity::Key::Skill {
            pane: ask.pane,
            ask: ask.id,
        };
        if matches!(ask.anchor, Anchor::Spot { .. }) || ask.answer.landed() {
            crate::activity::seen::watch(ctx, key, (room, at));
        }
        let chars = ask.asked_chars().max(ask.answer.text().chars().count());
        let bubble = Bubble {
            id: Id::new(("skill-card", ask.id.0)),
            at,
            room,
            width: width_for(chars),
            margin: Margin::ZERO,
            snug: false,
            prefer: Side::Below,
        };
        bubble.show(ctx, &t, |ui, cap| card(ui, ask, cap, &t, out));
    }
    indicators.show(ctx, out);
}

/// Where the card points, on screen: the asked text, or the page spot.
fn anchor_rect(ask: &Ask, docs: DocumentsView<'_>) -> Option<Rect> {
    match &ask.anchor {
        Anchor::Text(spans) => docs.spans_rect(ask.pane, spans),
        Anchor::Spot { page, x, y, .. } => {
            let sheet = docs.sheets(ask.pane).iter().find(|s| s.page == *page)?;
            let at = sheet.rect.min + vec2(*x, *y) * sheet.scale;
            Some(Rect::from_center_size(at, vec2(2.0, 2.0)))
        }
    }
}

fn card(ui: &mut Ui, ask: &Ask, cap: f32, t: &Tokens, out: &mut Commands) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let top = ui.cursor().top();
    header(ui, ask, t, out);
    word_card::rule(ui, t);
    let pad = word_card::PAD_Y + 2;
    let used = ui.cursor().top() - top + f32::from(pad) * 2.0 + word_card::FOOT;
    let most = (cap - used).clamp(BODY_LEAST, BODY_MOST);
    word_card::section(ui, pad, |ui| {
        ScrollArea::vertical()
            .id_salt(("skill-card-body", ask.id.0))
            .max_height(most)
            .auto_shrink([false, true])
            .stick_to_bottom(!ask.answer.landed())
            .show(ui, |ui| body(ui, ask, t));
    });
    word_card::rule(ui, t);
    footer(ui, ask, t, out);
}

/// The skill's name, and the button that closes the card.
fn header(ui: &mut Ui, ask: &Ask, t: &Tokens, out: &mut Commands) {
    word_card::footer_frame(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.add(Icon::Ai.image(ui, 14.0).tint(t.accent));
            ui.add_space(4.0);
            let name = RichText::new(&ask.name)
                .font(theme::medium(t.text_small))
                .color(t.ink);
            ui.add(egui::Label::new(name).truncate());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let close = widgets::small_icon_button(ui, Icon::Close, "Close");
                if close.clicked() {
                    out.push(Sent::Engine(Command::Skill(Action::Close(ask.id))));
                }
                if matches!(ask.anchor, Anchor::Text(_)) {
                    let label = match (ask.pinned, ask.pin_status) {
                        (None, _) => "Pin answer",
                        (Some(_), mareader_engine::library::SaveStatus::Pending) => {
                            "Saving answer…"
                        }
                        (Some(_), mareader_engine::library::SaveStatus::Failed) => {
                            "Save failed · unpin answer"
                        }
                        (Some(_), mareader_engine::library::SaveStatus::Saved) => "Unpin answer",
                    };
                    if widgets::icon_button(ui, Icon::Pin, label, ask.pinned.is_some()).clicked() {
                        out.push(Sent::Engine(Command::Skill(Action::Pin(ask.id))));
                    }
                    if widgets::small_icon_button(ui, Icon::Remove, "Remove answer").clicked() {
                        out.push(Sent::Engine(Command::Skill(Action::Remove(ask.id))));
                    }
                }
            });
        });
    });
}

fn body(ui: &mut Ui, ask: &Ask, t: &Tokens) {
    let size = t.text + TEXT_GROW;
    match &ask.answer {
        Answer::Waiting => shimmer(ui, t),
        Answer::Partial(text) | Answer::Ready(text) => {
            crate::shaped::label(ui, text.trim(), size, t.ink);
        }
        Answer::Interrupted => {
            ui.label(
                RichText::new("The app closed before this answer arrived. Try again.")
                    .color(t.muted),
            );
        }
        Answer::Failed(failure) => {
            ui.horizontal_wrapped(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                Icon::Alert.paint(ui, rect, t.danger);
                ui.label(RichText::new(failure.said()).color(t.muted));
            });
        }
    }
}

/// Who answered on the left; Copy and Ask again on the right once done,
/// or Try again after a failure that may pass.
fn footer(ui: &mut Ui, ask: &Ask, t: &Tokens, out: &mut Commands) {
    word_card::footer_frame(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let again = match &ask.answer {
                    Answer::Ready(text) => {
                        word_card::copy_button(ui, text.trim(), "Copy", out);
                        Some(("Ask again", false))
                    }
                    Answer::Failed(f) if f.why.retry() => Some(("Try again", true)),
                    Answer::Interrupted => Some(("Try again", true)),
                    _ => None,
                };
                if let Some((label, strong)) = again
                    && word_card::footer_action(ui, Some(Icon::Rescan), label, strong, t).clicked()
                {
                    out.push(Sent::Engine(Command::Skill(Action::Retry(ask.id))));
                }
                ui.add_space(6.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let said = match &ask.answer {
                        Answer::Waiting | Answer::Partial(_) => "Asking AI…",
                        Answer::Ready(_) => ask.by.as_str(),
                        Answer::Failed(_) | Answer::Interrupted => "",
                    };
                    let label = RichText::new(said).size(t.text_small).color(t.muted);
                    ui.add(egui::Label::new(label).truncate());
                });
            });
        });
    });
}

/// Three soft bars that breathe while the answer is on its way.
fn shimmer(ui: &mut Ui, t: &Tokens) {
    let phase = if motion::enabled(ui.ctx()) {
        ui.ctx().request_repaint();
        (ui.input(|i| i.time) * 3.0).sin() as f32 * 0.5 + 0.5
    } else {
        0.5
    };
    let tint = t.line.gamma_multiply(0.6 + 0.4 * phase);
    let width = ui.available_width().max(80.0);
    for (i, share) in [1.0, 0.92, 0.6].into_iter().enumerate() {
        if i > 0 {
            ui.add_space(8.0);
        }
        let (rect, _) = ui.allocate_exact_size(vec2(width, 12.0), egui::Sense::hover());
        let bar = Rect::from_min_size(rect.min, vec2(width * share, rect.height()));
        ui.painter().rect_filled(bar, 4.0, tint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_widens_with_its_text_and_then_stops() {
        assert_eq!(width_for(0), LEAST);
        assert_eq!(width_for(400), LEAST, "a sentence keeps the least");
        let (para, page) = (width_for(2_000), width_for(6_000));
        assert!(LEAST < para && para < page, "{para} {page}");
        assert_eq!(width_for(50_000), MOST);
    }
}
