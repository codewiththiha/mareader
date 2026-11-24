//! The deck at a pane's gate: the batch's new words as flash cards.
//!
//! A card's front holds the word alone and its band; turned over, it gives
//! the meaning and the word class, with the AI's synonyms and example. A
//! click or Space turns it, a drag or the arrows move on, K marks the word
//! known. Read now unlocks the batch whenever the reader likes, asking first
//! while time is left, as the cards then close for good; with a time limit
//! the deck also closes once its minutes are spent. Turning study mode off
//! from here asks first too. The pages, the time left and shuffle sit just
//! above the card, lined up with its edges; a short pane's deck drops that
//! row and shows the time left on the card itself.

use egui::{Align, Align2, Id, Key, Layout, Rect, RichText, Sense, Stroke, Ui};
use egui::{Vec2, vec2};
use mareader_engine::command::Command;
use mareader_engine::study_mode::{Action, DeckState, Open, Shown};
use mareader_study::mode::Batch;
use mareader_workspace::PaneId;

use super::confirm::Ask;
use super::controls::{self, Chip, Look};
use super::flip_card::{self, Faces, Gesture};
use super::rows;
use super::tight::{self, Fit};
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
/// Between the header's blocks and the card.
const HEADER_GAP: f32 = 12.0;
const NO_MEANING: &str = "No meaning was found for this word.";
/// What the time left in the header is for.
const LEFT_TIP: &str = "The cards close for these pages once it runs out";
/// The deck's widest; a narrow pane takes it in.
const WIDTH: f32 = 440.0;
/// A deck narrower than this is always compact: its roomy rows overflow.
const NARROW: f32 = 300.0;
/// A card's height to its width.
const ASPECT: f32 = 0.68;
/// The compact footer's gap, the room kept for the pane's corner marks,
/// the least Read now, and the least that keeps its icon.
const GAP: f32 = 8.0;
const MARKS: f32 = 72.0;
const LEAST_START: f32 = 84.0;
const ROOMY_START: f32 = 112.0;

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
pub(super) fn known(pane: PaneId, shown: &Shown, cursor: &mut Cursor, out: &mut Commands) {
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
    let fit = open.map(|open| plan(ui, pane, open, (room.height(), width)));
    let clear = fit.map_or(0.0, tight::head);
    let top = (room.center().y - tall / 2.0).max(room.top() + clear);
    let at = egui::pos2(room.center().x - width / 2.0, top);
    let column = Rect::from_min_size(at, vec2(width, room.bottom() - top));
    let builder = egui::UiBuilder::new()
        .max_rect(column)
        .layout(Layout::top_down(Align::Center));
    let used = ui.scope_builder(builder, |ui| {
        ui.set_width(width);
        if let (Some(open), Some(fit)) = (open, fit) {
            if !fit.scrolls {
                return whole(ui, pane, open, fit, (cursor, ask), out);
            }
            let wheel = egui::scroll_area::ScrollSource::MOUSE_WHEEL;
            egui::ScrollArea::vertical()
                .id_salt(("study-deck-scroll", pane.id))
                .max_height(room.bottom() - top - tight::FOOT)
                .scroll_source(wheel | egui::scroll_area::ScrollSource::SCROLL_BAR)
                .show(ui, |ui| whole(ui, pane, open, fit, (cursor, ask), out));
            return;
        }
        match &pane.run.deck {
            DeckState::Open(_) => {}
            DeckState::Failed { batch, why, setup } => {
                waiting::failed(ui, pane, &batch.label(), (why, *setup), out);
            }
            DeckState::Making { batch, job } => {
                waiting::waiting(ui, pane, &batch.label(), Some(job), out);
            }
            DeckState::None => {
                let range = pane.run.gate_batch().map_or_else(String::new, Batch::label);
                waiting::waiting(ui, pane, &range, None, out);
            }
        }
    });
    crate::activity::seen::deck(ui.ctx(), (pane.id, pane.run), used.response.rect);
    let height = used.response.rect.height();
    if (height - tall).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(tall_id, height));
        ui.ctx().request_discard("the deck's height changed");
    }
}

/// The open deck's layout in `room` points of height, `width` wide: its
/// card tall enough for the longest back, as far as the pane allows.
fn plan(ui: &Ui, pane: &Pane<'_>, open: &Open, (room, width): (f32, f32)) -> Fit {
    let t = Tokens::get(ui.ctx());
    let normal = card_size(width).y;
    let painter = ui.painter();
    let need = (open.cards.iter())
        .map(|shown| flip_card::need(painter, &t, &faces(shown), width))
        .fold(normal, f32::max);
    let chrome = tight::chrome(ui.ctx(), pane.id);
    let card = need.min(tight::TALLEST).max(normal);
    tight::plan(room - tight::FOOT, card, chrome, width < NARROW)
}

/// The open deck, header to footer, laid out by `fit`; keeps what it took.
fn whole(
    ui: &mut Ui,
    pane: &Pane<'_>,
    open: &Open,
    fit: Fit,
    (cursor, ask): (&mut Cursor, &mut Option<Ask>),
    out: &mut Commands,
) {
    let start = ui.next_widget_position().y;
    if !fit.compact {
        header(ui, (open, open.allowance.left(pane.now)), cursor);
    }
    deck(ui, pane, open, fit, (cursor, ask), out);
    if !open.cards.is_empty() {
        tight::measured(ui.ctx(), pane.id, fit, ui.min_rect().bottom() - start);
    }
}

/// A card's size in a column `width` wide.
pub fn card_size(width: f32) -> Vec2 {
    vec2(width, (width * ASPECT).min(300.0))
}

fn deck(
    ui: &mut Ui,
    pane: &Pane<'_>,
    open: &Open,
    fit: Fit,
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
        let size = vec2(ui.available_width(), fit.card);
        let card = Rect::from_min_size(ui.available_rect_before_wrap().min, size);
        let order = cursor.order.clone();
        let time = match fit.compact {
            true => left.map(clock).unwrap_or_default(),
            false => String::new(),
        };
        // Compact, the card itself says the time left and which card it is.
        let face = |i: usize| {
            let shown = order.get(i).and_then(|&c| open.cards.get(c))?;
            let (clock, at) = (time.clone(), i.saturating_add(1));
            let which = match fit.compact {
                true => format!("{at} / {count}"),
                false => String::new(),
            };
            Some(Faces {
                clock,
                count: which,
                ..faces(shown)
            })
        };
        let id = card_id(pane.id, cursor.batch);
        let shown = (cursor.at, count, cursor.turned);
        match flip_card::show(ui, id, size, shown, &face) {
            Some(Gesture::Turn) => cursor.turned = !cursor.turned,
            Some(Gesture::Step(by)) => cursor.step(by, count),
            None => {}
        }
        match fit.compact {
            true => rows::edges(ui, card, cursor, count),
            false => {
                ui.add_space(t.gap * 0.5);
                rows::steps(ui, cursor, count);
            }
        }
    }
    if !fit.compact || count == 0 {
        ui.add_space(t.gap * 1.25);
        rows::marks(ui, pane, open, cursor, out);
    }
    ui.add_space(t.gap * if fit.compact { 0.75 } else { 1.75 });
    footer(ui, pane, (open, left), (ask, fit.compact), (cursor, out));
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
        clock: String::new(),
        count: String::new(),
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

/// The row over the card: the pages at its left edge; the time left and
/// shuffle at its right.
fn header(ui: &mut Ui, (open, left): (&Open, Option<f64>), cursor: &mut Cursor) {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), controls::BLOCK);
    let (row, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.add_space(HEADER_GAP);
    let top = row.top();
    let pages = range(&open.batch);
    let time = left.map(clock);
    let (marks, shuffled) = crowd(ui, row.width(), (&pages, time.as_deref()), open);
    let mark = |icon| marks.then_some(icon);
    let at = row.left_top();
    let block = controls::block(ui, at, Align2::LEFT_TOP, mark(Icon::Book), &pages, &t);
    let resp = ui.interact(block, ui.id().with("deck-pages"), Sense::hover());
    let said = format!("The words before {}", open.batch.label());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &said));
    widgets::tip(&resp, &said, None);
    let mut right = row.right();
    let side = controls::BLOCK;
    if shuffled {
        let rect = Rect::from_min_size(egui::pos2(right - side, top), Vec2::splat(side));
        if shuffle(ui, rect, &t).clicked() {
            let seed = ui.input(|i| i.time).to_bits();
            cursor.shuffle(seed);
        }
        right -= side + 8.0;
    }
    if let Some(said) = time {
        let at = egui::pos2(right, top);
        let block = controls::block(ui, at, Align2::RIGHT_TOP, mark(Icon::Clock), &said, &t);
        let resp = ui.interact(block, ui.id().with("deck-clock"), Sense::hover());
        let name = format!("{said} left");
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
        widgets::tip(&resp, LEFT_TIP, None);
    }
}

/// What a header `width` wide holds: the blocks' marks, then shuffle, go
/// first when it is crowded.
fn crowd(ui: &Ui, width: f32, (pages, said): (&str, Option<&str>), open: &Open) -> (bool, bool) {
    let round = controls::BLOCK + 8.0;
    let take = |marks: bool| {
        let clock = said.map_or(0.0, |s| controls::block_width(ui, marks, s) + 8.0);
        controls::block_width(ui, marks, pages) + clock + 8.0
    };
    let many = open.cards.len() > 1;
    let fits = |marks: bool, shuffled: bool| {
        let shuffle = if shuffled { round } else { 0.0 };
        take(marks) + shuffle <= width
    };
    match (fits(true, many), fits(false, many)) {
        (true, _) => (true, many),
        (false, true) => (false, many),
        (false, false) => (false, false),
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

/// Read back and Read now side by side, then turning study off; compact,
/// the known and undo marks lead, and turning off is left to the page menu.
fn footer(
    ui: &mut Ui,
    pane: &Pane<'_>,
    (open, left): (&Open, Option<f64>),
    (ask, compact): (&mut Option<Ask>, bool),
    (cursor, out): (&mut Cursor, &mut Commands),
) {
    let tall = if compact { 34.0 } else { 40.0 };
    let back = match &pane.run.review {
        Some(review) => review.batch.from > 0,
        None => pane.run.back(pane.lock).is_some(),
    };
    let mut chips = match compact {
        true => rows::lead(open, cursor),
        false => Vec::new(),
    };
    let lead = chips.len();
    if back {
        let chip = Chip::new(READ_BACK, Look::Outline);
        chips.push(match compact {
            true => chip.bare(Icon::ChevronLeft).tall(tall),
            false => chip.icon(Icon::ChevronLeft).tall(tall).wide(150.0),
        });
    }
    let start = Chip::new(START, Look::Primary);
    let start = match compact {
        true => fill(ui, start, &chips),
        false => {
            let wide = if back { 150.0 } else { 200.0 };
            start.icon(Icon::Book).tall(tall).wide(wide)
        }
    };
    chips.push(start);
    let pressed = controls::row(ui, chips, if compact { GAP } else { 10.0 });
    if let Some(led) = pressed.get(..lead) {
        rows::led(led, pane, open, cursor, out);
    }
    if back && pressed.get(lead).is_some_and(egui::Response::clicked) {
        send(out, Action::ReadPrevious(pane.id));
    }
    if pressed.last().is_some_and(egui::Response::clicked) {
        let range = open.batch.label();
        match left.filter(|&l| l > 0.0 && pane.run.review.is_none()) {
            Some(left) => *ask = Some(Ask::ReadNow { range, left }),
            None => send(out, Action::Start(pane.id)),
        }
    }
    if compact {
        return;
    }
    ui.add_space(6.0);
    let off = Chip::new(TURN_OFF, Look::Quiet).tall(30.0);
    let pressed = controls::row(ui, vec![off], 0.0);
    if pressed.iter().any(egui::Response::clicked) {
        *ask = Some(Ask::TurnOff);
    }
}

/// Compact Read now: as wide as the row leaves, clear of the corner marks
/// when it can be, its icon kept when there is room.
fn fill<'a>(ui: &Ui, start: Chip<'a>, before: &[Chip<'_>]) -> Chip<'a> {
    let taken: f32 = before.iter().map(|c| c.measure(ui) + GAP).sum();
    let width = ui.available_width();
    let clear = width - MARKS - taken;
    let room = match clear >= LEAST_START {
        true => clear,
        false => width - taken,
    };
    let start = start.tall(34.0).wide(room.clamp(LEAST_START, 200.0));
    match room >= ROOMY_START {
        true => start.icon(Icon::Book),
        false => start,
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
