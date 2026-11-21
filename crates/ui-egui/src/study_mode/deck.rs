//! The deck at a pane's gate: the batch's new words as flash cards.
//!
//! A card's front holds the word alone and its band; turned over, it gives
//! the meaning and the word class, with the AI's synonyms and example. A
//! click or Space turns it, a drag or the arrows move on, K marks the word
//! known. Read now unlocks the batch whenever the reader likes, asking first
//! while time is left, as the cards then close for good; with a time limit
//! the deck also closes once its minutes are spent. Turning study mode off
//! from here asks first too. The pages, the time left and shuffle sit in
//! small blocks in the pane's top corners.

use egui::{Align, Align2, Frame, Id, Key, Layout, Margin, Rect, RichText, Sense, Stroke, Ui};
use egui::{Vec2, vec2};
use mareader_engine::command::Command;
use mareader_engine::study_mode::{Action, DeckState, Open, Shown};
use mareader_study::mode::Batch;
use mareader_workspace::PaneId;

use super::confirm::Ask;
use super::controls::{self, Chip, Look};
use super::flip_card::{self, Faces, Gesture};
use super::{Cursor, Pane, waiting};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;

pub const KNOWN: &str = "I already know this";
pub const START: &str = "Read now";
pub const SHUFFLE: &str = "Shuffle";
pub const TURN_OFF: &str = "Turn off study mode";
pub const READ_BACK: &str = "Read back";
const PREVIOUS: &str = "Previous card";
const NEXT: &str = "Next card";
const UNDO: &str = "Undo known";
/// The room the counter takes between the step buttons.
const STEP_GAP: f32 = 96.0;
/// The corner blocks' drop below the pane's top, clear of its label.
const CORNER_DROP: f32 = 22.0;
/// The room the corner blocks keep over the deck.
const HEAD: f32 = CORNER_DROP + controls::BLOCK + 12.0;
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

/// The deck in the middle of `ui`, which covers the pane, centred by the
/// height it took last pass; what it asks before acting goes in `ask`.
pub fn show(
    ui: &mut Ui,
    pane: &Pane<'_>,
    cursor: &mut Cursor,
    ask: &mut Option<Ask>,
    out: &mut Commands,
) {
    let room = ui.max_rect();
    let width = WIDTH.min(room.width());
    let tall_id = Id::new(("study-deck-tall", pane.id));
    let kept = ui.ctx().data(|d| d.get_temp(tall_id));
    let tall: f32 = kept.unwrap_or(room.height());
    let open = pane.run.shown_deck();
    let clear = if open.is_some() { HEAD } else { 0.0 };
    let top = (room.center().y - tall / 2.0).max(room.top() + clear);
    let at = egui::pos2(room.center().x - width / 2.0, top);
    let column = Rect::from_min_size(at, vec2(width, room.bottom() - top));
    let builder = egui::UiBuilder::new()
        .max_rect(column)
        .layout(Layout::top_down(Align::Center));
    let used = ui.scope_builder(builder, |ui| {
        ui.set_width(width);
        if let Some(open) = open {
            corners(ui, room, (open, open.allowance.left(pane.now)), cursor);
            deck(ui, pane, open, (cursor, ask), out);
            return;
        }
        match &pane.run.deck {
            DeckState::Open(_) => {}
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
    let height = used.response.rect.height();
    if (height - tall).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(tall_id, height));
        ui.ctx().request_discard("the deck's height changed");
    }
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

fn deck(
    ui: &mut Ui,
    pane: &Pane<'_>,
    open: &Open,
    (cursor, ask): (&mut Cursor, &mut Option<Ask>),
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let left = open.allowance.left(pane.now);
    let count = open.cards.len();
    cursor.fit(count);
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
        ui.add_space(t.gap * 0.5);
        steps(ui, cursor, count);
    }
    ui.add_space(t.gap * 1.25);
    marks(ui, pane, open, cursor, out);
    ui.add_space(t.gap * 1.75);
    footer(ui, pane, (open, left), ask, out);
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

/// The pages as bare numbers: "3" or "3 – 5".
pub fn range(batch: &Batch) -> String {
    let (from, to) = (batch.from.saturating_add(1), batch.to.saturating_add(1));
    match from == to {
        true => from.to_string(),
        false => format!("{from} – {to}"),
    }
}

/// The blocks in the pane's top corners: the pages at the left; the time
/// left and shuffle at the right, each on its own.
fn corners(ui: &mut Ui, room: Rect, (open, left): (&Open, Option<f64>), cursor: &mut Cursor) {
    let t = Tokens::get(ui.ctx());
    let top = room.top() + CORNER_DROP;
    let pages = range(&open.batch);
    let at = egui::pos2(room.left(), top);
    let block = controls::block(ui, at, Align2::LEFT_TOP, Some(Icon::Book), &pages, &t);
    let resp = ui.interact(block, ui.id().with("deck-pages"), Sense::hover());
    let said = format!("The words before {}", open.batch.label());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &said));
    widgets::tip(&resp, &said, None);
    let mut right = room.right();
    if open.cards.len() > 1 {
        let side = controls::BLOCK;
        let rect = Rect::from_min_size(egui::pos2(right - side, top), Vec2::splat(side));
        if shuffle(ui, rect, &t).clicked() {
            let seed = ui.input(|i| i.time).to_bits();
            cursor.shuffle(seed);
        }
        right -= side + 8.0;
    }
    if let Some(left) = left {
        let at = egui::pos2(right, top);
        let said = clock(left);
        let block = controls::block(ui, at, Align2::RIGHT_TOP, Some(Icon::Clock), &said, &t);
        let resp = ui.interact(block, ui.id().with("deck-clock"), Sense::hover());
        let name = format!("{said} left");
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
        widgets::tip(&resp, LEFT_TIP, None);
    }
}

/// A round block with two crossing arrows: the order shuffled.
fn shuffle(ui: &mut Ui, rect: Rect, t: &Tokens) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with("deck-shuffle"), Sense::click());
    controls::plate(ui, rect, t);
    if resp.hovered() {
        let r = rect.height() / 2.0 - 1.0;
        ui.painter().circle_filled(rect.center(), r, t.hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let ink = if resp.hovered() { t.ink } else { t.muted };
    let stroke = Stroke::new(1.6, ink);
    let p = |x: f32, y: f32| rect.center() + vec2(x, y) * 0.8;
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

/// Back and forward between cards, with where this one sits between.
fn steps(ui: &mut Ui, cursor: &mut Cursor, count: usize) {
    let t = Tokens::get(ui.ctx());
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
    let pressed = controls::row(ui, vec![back, next], STEP_GAP);
    if pressed.first().is_some_and(egui::Response::clicked) {
        cursor.step(-1, count);
    }
    if pressed.get(1).is_some_and(egui::Response::clicked) {
        cursor.step(1, count);
    }
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
fn marks(ui: &mut Ui, pane: &Pane<'_>, open: &Open, cursor: &mut Cursor, out: &mut Commands) {
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
        widgets::tip(mark, "Skipped in every book from now on", Some("K"));
    }
    if let (Some(shown), true) = (shown, pressed.first().is_some_and(egui::Response::clicked)) {
        known(pane.id, shown, cursor, out);
    } else if undo && pressed.last().is_some_and(egui::Response::clicked) {
        send(out, Action::Undo(pane.id));
    }
}

/// Read back and Read now side by side, then turning study off.
fn footer(
    ui: &mut Ui,
    pane: &Pane<'_>,
    (open, left): (&Open, Option<f64>),
    ask: &mut Option<Ask>,
    out: &mut Commands,
) {
    let back = match &pane.run.review {
        Some(review) => review.batch.from > 0,
        None => pane.run.back(pane.lock).is_some(),
    };
    let mut chips = Vec::new();
    if back {
        let chip = Chip::new(READ_BACK, Look::Outline).icon(Icon::ChevronLeft);
        chips.push(chip.tall(40.0).wide(150.0));
    }
    let width = if back { 150.0 } else { 200.0 };
    let start = Chip::new(START, Look::Primary).icon(Icon::Book);
    chips.push(start.tall(40.0).wide(width));
    let pressed = controls::row(ui, chips, 10.0);
    if back && pressed.first().is_some_and(egui::Response::clicked) {
        send(out, Action::ReadPrevious(pane.id));
    }
    if pressed.last().is_some_and(egui::Response::clicked) {
        let range = open.batch.label();
        match left.filter(|&l| l > 0.0 && pane.run.review.is_none()) {
            Some(left) => *ask = Some(Ask::ReadNow { range, left }),
            None => send(out, Action::Start(pane.id)),
        }
    }
    ui.add_space(6.0);
    let off = Chip::new(TURN_OFF, Look::Quiet).tall(30.0);
    let pressed = controls::row(ui, vec![off], 0.0);
    if pressed.iter().any(egui::Response::clicked) {
        *ask = Some(Ask::TurnOff);
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
