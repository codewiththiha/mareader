//! The deck at a pane's gate: the batch's new words as flash cards.
//!
//! A card's front holds the word alone and its band; turned over, it gives
//! the meaning and the word class, with the AI's synonyms and example. A
//! click or Space turns it, a drag or the arrows move on, K marks the word
//! known. Read now unlocks the batch whenever the reader likes; with a time
//! limit the deck also closes once its minutes are spent.

use egui::{Align, Frame, Id, Key, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2, vec2};
use mareader_engine::command::Command;
use mareader_engine::study_mode::{Action, DeckState, Open, Shown};
use mareader_study::mode::Batch;
use mareader_workspace::PaneId;

use super::flip_card::{self, Faces, Gesture};
use super::{Cursor, Pane, waiting};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;

pub const KNOWN: &str = "I already know this";
pub const START: &str = "Read now";
pub const SHUFFLE: &str = "Shuffle";
const NO_MEANING: &str = "No meaning was found for this word.";
/// What the time left in the header is for.
const LEFT_TIP: &str = "The cards close for these pages once it runs out";
/// The deck's widest; a narrow pane takes it in.
const WIDTH: f32 = 440.0;
/// A card's height to its width.
const ASPECT: f32 = 0.68;

/// A key the deck answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Turn,
    Known,
    Step(isize),
}

impl Press {
    /// Takes the first deck key pressed this frame, so the pane never sees it.
    pub fn read(input: &mut egui::InputState) -> Option<Self> {
        let none = egui::Modifiers::NONE;
        let keys = [
            (Key::Space, Self::Turn),
            (Key::Enter, Self::Turn),
            (Key::K, Self::Known),
            (Key::ArrowLeft, Self::Step(-1)),
            (Key::ArrowRight, Self::Step(1)),
        ];
        for (key, press) in keys {
            if input.consume_key(none, key) {
                return Some(press);
            }
        }
        None
    }
}

/// Acts on `press` in `pane`'s open deck.
pub fn press(cursor: &mut Cursor, open: &Open, pane: PaneId, press: Press, out: &mut Commands) {
    let count = open.cards.len();
    cursor.fit(count);
    match press {
        Press::Turn => cursor.turned = count > 0 && !cursor.turned,
        Press::Step(by) => cursor.step(by, count),
        Press::Known => {
            if let Some(shown) = cursor.card().and_then(|i| open.cards.get(i)) {
                known(pane, shown, cursor, out);
            }
        }
    }
}

/// Marks `shown` known; the card after it takes its place, face up.
fn known(pane: PaneId, shown: &Shown, cursor: &mut Cursor, out: &mut Commands) {
    if let Some(card) = cursor.card() {
        cursor.drop_card(card);
    }
    let key = shown.card.key.clone();
    send(out, Action::Known { pane, key });
}

pub(super) fn send(out: &mut Commands, action: Action) {
    out.push(Sent::Engine(Command::StudyMode(action)));
}

/// The deck in the middle of `ui`, which covers the pane.
pub fn show(ui: &mut Ui, pane: &Pane<'_>, cursor: &mut Cursor, out: &mut Commands) {
    let room = ui.max_rect();
    let width = WIDTH.min(room.width());
    let at = room.center_top() + vec2(-width / 2.0, (room.height() * 0.08).min(48.0));
    let builder = egui::UiBuilder::new()
        .max_rect(egui::Rect::from_min_size(at, vec2(width, room.height())))
        .layout(Layout::top_down(Align::Center));
    ui.scope_builder(builder, |ui| {
        ui.set_width(width);
        match &pane.run.deck {
            DeckState::Open(open) => deck(ui, pane, open, cursor, out),
            DeckState::Failed { batch, why } => framed(ui, |ui| {
                waiting::failed(ui, pane, &batch.label(), why, out);
            }),
            DeckState::Making { batch, job } => framed(ui, |ui| {
                waiting::waiting(ui, pane, &batch.label(), Some(job), out);
            }),
            DeckState::None => framed(ui, |ui| {
                let range = pane.run.gate_batch().map_or_else(String::new, Batch::label);
                waiting::waiting(ui, pane, &range, None, out);
            }),
        }
    });
}

/// The waiting and error cards' paper.
fn framed(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width();
    Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius_lg)
        .inner_margin(Margin::same(18))
        .shadow(egui::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: t.shadow,
        })
        .show(ui, |ui| {
            ui.set_width(width - 36.0);
            ui.with_layout(Layout::top_down(Align::Min), content);
        });
}

fn deck(ui: &mut Ui, pane: &Pane<'_>, open: &Open, cursor: &mut Cursor, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let left = open.allowance.left(pane.now);
    let count = open.cards.len();
    cursor.fit(count);
    header(ui, open, left, cursor);
    ui.add_space(t.gap * 1.5);
    if count == 0 {
        let all = "You know every word in these pages.";
        ui.label(RichText::new(all).font(theme::serif(16.0)));
    } else {
        let width = ui.available_width();
        let size = vec2(width, (width * ASPECT).min(300.0));
        let order = cursor.order.clone();
        let face = |i: usize| order.get(i).and_then(|&c| open.cards.get(c)).map(faces);
        let id = card_id(pane.id, cursor.batch);
        let shown = (cursor.at, count, cursor.turned);
        match flip_card::show(ui, id, size, shown, &face) {
            Some(Gesture::Turn) => cursor.turned = !cursor.turned,
            Some(Gesture::Step(by)) => cursor.step(by, count),
            None => {}
        }
        steps(ui, cursor, count);
    }
    ui.add_space(t.gap);
    marks(ui, pane, open, cursor, out);
    ui.add_space(t.gap);
    footer(ui, pane, out);
    if let Some(left) = left.filter(|_| open.allowance.running()) {
        let tick = left.fract().max(0.05) as f32;
        ui.ctx().request_repaint_after_secs(tick);
    }
}

/// The flip card's id in `pane`, for the batch from page to page.
pub fn card_id(pane: PaneId, batch: (usize, usize)) -> Id {
    Id::new(("study-card", pane, batch))
}

/// A card's faces: the word and band, then what it means.
pub fn faces(shown: &Shown) -> Faces {
    let card = &shown.card;
    let meaning = match shown.meaning.is_empty() {
        true => NO_MEANING,
        false => shown.meaning.as_str(),
    };
    let synonyms = match shown.synonyms.is_empty() {
        true => String::new(),
        false => format!("Like: {}", shown.synonyms.join(", ")),
    };
    Faces {
        word: card.key.lemma.clone(),
        badge: mareader_cefr::label(card.band).to_owned(),
        meaning: meaning.to_owned(),
        class: card.key.pos.map_or("", |p| p.name()).to_owned(),
        synonyms,
        usage: shown.usage.clone(),
    }
}

/// The capsule over the cards: the pages, the count, the time left, shuffle.
fn header(ui: &mut Ui, open: &Open, left: Option<f64>, cursor: &mut Cursor) {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width();
    Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(24.0)
        .inner_margin(Margin::symmetric(14, 8))
        .shadow(egui::Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: t.shadow,
        })
        .show(ui, |ui| {
            ui.set_width(width - 28.0);
            ui.horizontal(|ui| {
                let lead = format!("Before {}", open.batch.label());
                ui.label(RichText::new(lead).font(theme::medium(15.0)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if open.cards.len() > 1 && shuffle(ui, &t).clicked() {
                        let seed = ui.input(|i| i.time).to_bits();
                        cursor.shuffle(seed);
                    }
                    if let Some(left) = left {
                        let said = format!("{} left", clock(left));
                        let clock = RichText::new(said).size(t.text_small).color(t.muted);
                        ui.label(clock).on_hover_text(LEFT_TIP);
                    }
                    let n = open.cards.len().to_string();
                    ui.label(RichText::new(n).size(15.0).color(t.muted));
                });
            });
        });
}

/// Two crossing arrows: the order shuffled.
fn shuffle(ui: &mut Ui, t: &Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    let ink = if resp.hovered() { t.ink } else { t.muted };
    let stroke = Stroke::new(1.6, ink);
    let p = |x: f32, y: f32| rect.center() + vec2(x, y);
    let painter = ui.painter();
    let lanes = [(-5.0, 5.0), (5.0, -5.0)];
    for (from, to) in lanes {
        let lane = vec![p(-8.0, from), p(-3.0, from), p(3.0, to), p(8.0, to)];
        painter.add(egui::Shape::line(lane, stroke));
        painter.line_segment([p(8.0, to), p(5.0, to - 3.0)], stroke);
        painter.line_segment([p(8.0, to), p(5.0, to + 3.0)], stroke);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, SHUFFLE));
    widgets::tip(&resp, SHUFFLE, None);
    resp
}

/// Seconds as m:ss, rounded up so 0:00 shows only once done.
pub fn clock(secs: f64) -> String {
    let whole = secs.max(0.0).ceil().min(f64::from(u32::MAX)) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// Back and forward between cards, where this one sits, and how to move.
fn steps(ui: &mut Ui, cursor: &mut Cursor, count: usize) {
    let t = Tokens::get(ui.ctx());
    let at = cursor.at.saturating_add(1);
    let said = format!("{at} of {count} · drag or swipe, press to turn");
    let text = RichText::new(said).size(t.text_small).color(t.muted);
    let font = egui::FontId::proportional(t.text_small);
    let galley = ui
        .painter()
        .layout_no_wrap(text.text().to_owned(), font, t.muted);
    let wide = galley.rect.width() + 2.0 * 24.0 + 2.0 * ui.spacing().item_spacing.x;
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - wide) / 2.0).max(0.0));
        let first = cursor.at == 0;
        let back = widgets::small_icon_button(ui, Icon::ChevronLeft, "Previous card");
        if back.clicked() && !first {
            cursor.step(-1, count);
        }
        ui.label(text);
        let next = widgets::small_icon_button(ui, Icon::ChevronRight, "Next card");
        if next.clicked() {
            cursor.step(1, count);
        }
    });
}

/// Know this word, take that back, or read the pages before.
fn marks(ui: &mut Ui, pane: &Pane<'_>, open: &Open, cursor: &mut Cursor, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    if let Some(shown) = cursor.card().and_then(|i| open.cards.get(i)) {
        let label = RichText::new(KNOWN).size(t.text_small + 1.0);
        let mark = ui.add(egui::Button::new(label).frame_when_inactive(false));
        widgets::tip(&mark, "Skipped in every book from now on", Some("K"));
        if mark.clicked() {
            known(pane.id, shown, cursor, out);
        }
    }
    let undo = open.can_undo();
    let back = pane.run.back(pane.lock).is_some();
    if !undo && !back {
        return;
    }
    ui.horizontal(|ui| {
        let wide = 24.0 + 90.0;
        ui.add_space(((ui.available_width() - wide) / 2.0).max(0.0));
        if undo && widgets::small_icon_button(ui, Icon::Restore, "Undo known").clicked() {
            send(out, Action::Undo(pane.id));
        }
        if back && ui.button("Read back").clicked() {
            send(out, Action::ReadPrevious(pane.id));
        }
    });
}

fn footer(ui: &mut Ui, pane: &Pane<'_>, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let start = widgets::primary_button(ui, None, START, t.accent, &t);
    if ui.add(start.min_size(vec2(160.0, 32.0))).clicked() {
        send(out, Action::Start(pane.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_rounds_up_to_whole_seconds() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(0.2), "0:01");
        assert_eq!(clock(72.0), "1:12");
        assert_eq!(clock(-4.0), "0:00");
        assert_eq!(clock(f64::NAN), "0:00");
    }

    #[test]
    fn deck_keys_are_taken_once() {
        let mut input = egui::InputState::default();
        input.events.push(egui::Event::Key {
            key: Key::K,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        assert_eq!(Press::read(&mut input), Some(Press::Known));
        assert_eq!(Press::read(&mut input), None);
    }
}
