//! The deck at a pane's gate: the batch's new words, one card at a time.
//!
//! A card shows its word, its band and the sentence it came from; its
//! meaning stays hidden until revealed. Start unlocks the batch once the
//! time floor runs out (or at once, with the floor off). Keys: Space reveals,
//! K marks the word known, the arrows step between cards.

use egui::{Align, Frame, Key, Layout, Margin, RichText, Sense, Stroke, Ui, Vec2, vec2};
use mareader_engine::command::Command;
use mareader_engine::study_mode::{Action, DeckState, Open, Shown};
use mareader_study::mode::{Batch, Card};
use mareader_workspace::PaneId;

use super::{Cursor, Pane};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;

/// The reveal button's label, for screen readers and tests.
pub const REVEAL: &str = "Think of the meaning, then reveal";
pub const KNOWN: &str = "I already know this";
pub const START: &str = "Start reading";
/// The deck's widest; a narrow pane takes it in.
const WIDTH: f32 = 440.0;

/// A key the deck answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Reveal,
    Known,
    Step(isize),
}

impl Press {
    /// Takes the first deck key pressed this frame, so the pane never sees it.
    pub fn read(input: &mut egui::InputState) -> Option<Self> {
        let none = egui::Modifiers::NONE;
        let keys = [
            (Key::Space, Self::Reveal),
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
    match press {
        Press::Reveal => cursor.revealed = count > 0,
        Press::Step(by) => cursor.step(by, count),
        Press::Known => {
            if let Some(shown) = open.cards.get(cursor.at) {
                known(pane, shown, cursor, out);
            }
        }
    }
}

/// Marks `shown` known; the card after it takes its place, hidden.
fn known(pane: PaneId, shown: &Shown, cursor: &mut Cursor, out: &mut Commands) {
    cursor.revealed = false;
    let key = shown.card.key.clone();
    send(out, Action::Known { pane, key });
}

fn send(out: &mut Commands, action: Action) {
    out.push(Sent::Engine(Command::StudyMode(action)));
}

/// The deck in the middle of `ui`, which covers the pane.
pub fn show(ui: &mut Ui, pane: &Pane<'_>, cursor: &mut Cursor, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let room = ui.max_rect();
    let width = WIDTH.min(room.width());
    let frame = Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius_lg)
        .inner_margin(Margin::same(18))
        .shadow(egui::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: t.shadow,
        });
    let at = room.center_top() + vec2(-width / 2.0, (room.height() * 0.12).min(64.0));
    let builder = egui::UiBuilder::new()
        .max_rect(egui::Rect::from_min_size(at, vec2(width, room.height())))
        .layout(Layout::top_down(Align::Min));
    ui.scope_builder(builder, |ui| {
        frame.show(ui, |ui| {
            ui.set_width(width - 36.0);
            match &pane.run.deck {
                DeckState::Open(open) => deck(ui, pane, open, cursor, out),
                DeckState::Failed { batch, why } => failed(ui, pane, batch, why, out),
                DeckState::Making { batch, .. } => making(ui, &batch.label()),
                DeckState::None => {
                    let range = pane.run.gate_batch().map_or_else(String::new, Batch::label);
                    making(ui, &range);
                }
            }
        });
    });
}

fn making(ui: &mut Ui, range: &str) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(12.0));
        let text = format!("Preparing the words for {range}");
        ui.label(RichText::new(text).size(t.text).color(t.muted));
    });
}

fn failed(ui: &mut Ui, pane: &Pane<'_>, batch: &Batch, why: &str, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let title = format!("The AI couldn’t write the words for {}", batch.label());
    ui.label(RichText::new(title).font(theme::medium(t.text)));
    ui.label(RichText::new(why).size(t.text_small).color(t.muted));
    ui.add_space(t.gap);
    ui.horizontal(|ui| {
        let again = widgets::primary_button(ui, Some(Icon::Rescan), "Try again", t.accent, &t);
        if ui.add(again).clicked() {
            send(out, Action::Retry(pane.id));
        }
        if pane.run.previous().is_some() && ui.button("Read previous pages").clicked() {
            send(out, Action::ReadPrevious(pane.id));
        }
    });
}

fn deck(ui: &mut Ui, pane: &Pane<'_>, open: &Open, cursor: &mut Cursor, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let left = open.floor.left(pane.now);
    header(ui, pane, open, left);
    ui.add_space(t.gap * 1.5);
    let count = open.cards.len();
    cursor.at = cursor.at.min(count.saturating_sub(1));
    match open.cards.get(cursor.at) {
        Some(shown) => card(ui, pane.id, shown, cursor, out),
        None => {
            let all = "You know every word in these pages.";
            ui.label(RichText::new(all).font(theme::serif(16.0)));
        }
    }
    if count > 1 {
        steps(ui, cursor, count);
    }
    ui.add_space(t.gap);
    widgets::menu_rule(ui);
    let note = "Words you mark known are skipped in every book, from now on.";
    ui.label(RichText::new(note).size(t.text_small).color(t.muted));
    ui.add_space(t.gap);
    footer(ui, pane, open, left, out);
    if left > 0.0 {
        let tick = left.fract().max(0.05) as f32;
        ui.ctx().request_repaint_after_secs(tick);
    }
}

fn header(ui: &mut Ui, pane: &Pane<'_>, open: &Open, left: f64) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            let of = pane.run.batches.len();
            let index = open.batch.index.saturating_add(1);
            let title = format!("Batch {index} of {of} · {}", open.batch.label());
            ui.label(RichText::new(title).size(t.text_small).color(t.muted));
            let n = open.cards.len();
            let words = if n == 1 { "word" } else { "words" };
            let lead = format!("{n} new {words} before you read on");
            ui.label(RichText::new(lead).font(theme::medium(16.0)));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ring(ui, open.floor.progress(pane.now), left, &t);
        });
    });
}

/// The time floor as a ring filling up, with what is left inside.
fn ring(ui: &mut Ui, progress: f32, left: f64, t: &Tokens) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
    let label = clock(left);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
    let c = rect.center();
    let r = 17.0;
    let painter = ui.painter();
    painter.circle_stroke(c, r, Stroke::new(3.0, t.track));
    let steps = 48;
    let done = (progress.clamp(0.0, 1.0) * steps as f32).round() as usize;
    let at = |i: usize| {
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / steps as f32;
        c + r * Vec2::angled(a)
    };
    let arc: Vec<_> = (0..=done).map(at).collect();
    if arc.len() > 1 {
        painter.add(egui::Shape::line(arc, Stroke::new(3.0, t.accent)));
    }
    let font = theme::medium(10.5);
    painter.text(c, egui::Align2::CENTER_CENTER, label, font, t.ink);
}

/// Seconds as m:ss, rounded up so 0:00 shows only once done.
pub fn clock(secs: f64) -> String {
    let whole = secs.max(0.0).ceil().min(f64::from(u32::MAX)) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

fn card(ui: &mut Ui, pane: PaneId, shown: &Shown, cursor: &mut Cursor, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let card = &shown.card;
    Frame::new()
        .fill(t.card)
        .corner_radius(t.radius)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(&card.key.lemma).font(theme::serif(26.0)));
                let pos = card.key.pos.map_or("", |p| p.name());
                let band = mareader_cefr::label(card.band);
                let tags = [pos, band].into_iter().filter(|s| !s.is_empty());
                let tags = tags.collect::<Vec<_>>().join(" · ");
                ui.label(RichText::new(tags).size(t.text_small).color(t.muted));
            });
            ui.label(RichText::new(seen(card)).size(t.text_small).color(t.muted));
            ui.add_space(t.gap);
            let from = RichText::new("From the book").size(t.text_small);
            ui.label(from.color(t.muted));
            let sentence = RichText::new(&card.sentence).italics();
            ui.label(sentence.font(theme::serif(14.0)));
            ui.add_space(t.gap);
            if cursor.revealed {
                ui.label(RichText::new(&shown.meaning).size(t.text).color(t.ink));
            } else {
                let reveal = ui.add(egui::Button::new(REVEAL).min_size(vec2(0.0, 28.0)));
                widgets::tip(&reveal, REVEAL, Some("Space"));
                if reveal.clicked() {
                    cursor.revealed = true;
                }
            }
        });
    ui.add_space(t.gap);
    let mark = ui.add(egui::Button::new(KNOWN).frame_when_inactive(false));
    widgets::tip(&mark, KNOWN, Some("K"));
    if mark.clicked() {
        known(pane, shown, cursor, out);
    }
}

/// How often the word shows in the batch, and its other classes in the deck.
pub fn seen(card: &Card) -> String {
    let mut seen = format!("Appears {}×", card.count);
    let also: Vec<&str> = card.also.iter().map(|pos| pos.name()).collect();
    if !also.is_empty() {
        seen.push_str(&format!(" · also as {}", also.join(", ")));
    }
    seen
}

/// Back and forward between cards, and where this one sits.
fn steps(ui: &mut Ui, cursor: &mut Cursor, count: usize) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        if widgets::small_icon_button(ui, Icon::ChevronLeft, "Previous card").clicked() {
            cursor.step(-1, count);
        }
        let at = format!("{} / {count}", cursor.at.saturating_add(1));
        ui.label(RichText::new(at).size(t.text_small).color(t.muted));
        if widgets::small_icon_button(ui, Icon::ChevronRight, "Next card").clicked() {
            cursor.step(1, count);
        }
    });
}

fn footer(ui: &mut Ui, pane: &Pane<'_>, open: &Open, left: f64, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let id = pane.id;
    ui.horizontal(|ui| {
        if open.can_undo() {
            let undo = widgets::small_icon_button(ui, Icon::Restore, "Undo known");
            if undo.clicked() {
                send(out, Action::Undo(id));
            }
        }
        if pane.run.previous().is_some() && ui.button("Read back").clicked() {
            send(out, Action::ReadPrevious(id));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let locked = open.floor.locked(pane.strict, pane.now);
            let label = match locked {
                true => format!("{START} · {}", clock(left)),
                false => START.to_owned(),
            };
            let start = widgets::primary_button(ui, None, &label, t.accent, &t);
            let start = ui.add_enabled(!locked, start);
            if start.clicked() {
                send(out, Action::Start(id));
            }
        });
    });
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
