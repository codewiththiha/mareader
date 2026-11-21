//! The deck's place while it is made, and when it couldn't be.
//!
//! While it is made, blank cards drift over the frost with the stage the
//! deck is at beneath; once the AI has taken [`SLOW_SECS`] it says so and
//! offers the dictionary. A failure offers another try, the dictionary and
//! the pages before the gate.

use egui::{Align, Label, Rect, Response, RichText, Sense, Ui, Vec2, vec2};
use mareader_engine::study_mode::{Action, DeckJob, Doing};
use mareader_study::mode::{SLOW_SECS, Via};

use super::controls::{self, Chip, Look};
use super::deck::{self, send};
use super::{Pane, making};
use crate::icons::Icon;
use crate::motion;
use crate::theme::{self, Tokens};
use crate::ui_command::Commands;

/// The buttons' labels, for screen readers and tests.
pub const DICTIONARY: &str = "Use the dictionary instead";
pub const RETRY: &str = "Try again";
pub const PREVIOUS: &str = "Read previous pages";
/// What the waiting card says once the AI is slow.
pub const SLOW: &str = "This is taking longer than usual. You can read the previous pages \
while it finishes, and the deck opens as soon as it's ready.";
const USUAL: &str = "Words are prepared a batch ahead while you read, so you rarely wait. \
You can scroll back in the meantime.";
/// The widest the notes run, and the progress track's width.
const NOTE_W: f32 = 420.0;
const BAR_W: f32 = 220.0;

/// What the deck is doing now, in words.
pub fn stage(range: &str, job: Option<&DeckJob>) -> String {
    let Some(job) = job else {
        return format!("Finding the new words in {range}…");
    };
    let skipped = match job.skipped() {
        0 => String::new(),
        n => format!(" Set aside {n} you already memorized."),
    };
    match job.doing() {
        Doing::Finding => format!("Finding the new words in {range}…"),
        Doing::Writing => {
            let by = job.by().filter(|b| !b.is_empty());
            let by = by.unwrap_or_else(|| "the AI".to_owned());
            format!("Writing meanings in context with {by}…{skipped}")
        }
        Doing::LookingUp => format!("Looking them up in the offline dictionary…{skipped}"),
    }
}

/// Whether the AI kept the deck long enough to offer the dictionary.
pub fn slow(job: &DeckJob, now: f64) -> bool {
    job.via() == Via::Ai && job.waited(now) > SLOW_SECS
}

/// The cards being made for `range`, with what the deck is doing.
pub fn waiting(
    ui: &mut Ui,
    pane: &Pane<'_>,
    range: &str,
    job: Option<&DeckJob>,
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    making::show(ui, deck::card_size(ui.available_width()));
    ui.add_space(t.gap * 2.0);
    let title = format!("Preparing the words for {range}");
    let font = theme::semibold(20.0);
    centred(ui, RichText::new(title).font(font).color(t.ink));
    ui.add_space(2.0);
    let said = stage(range, job);
    centred(ui, RichText::new(said).size(t.text).color(t.muted));
    ui.add_space(t.gap * 1.25);
    bar(ui, job.and_then(written), &t);
    ui.add_space(t.gap * 1.25);
    let slow = job.is_some_and(|j| slow(j, pane.now));
    let note = if slow { SLOW } else { USUAL };
    ui.allocate_ui(vec2(NOTE_W.min(ui.available_width()), 0.0), |ui| {
        centred(ui, RichText::new(note).size(t.text_small).color(t.muted));
    });
    let back = pane.run.back(pane.lock).is_some();
    let dict = slow && pane.dict;
    let mut chips = Vec::new();
    if back {
        chips.push(Chip::new(PREVIOUS, Look::Outline).icon(Icon::ChevronLeft));
    }
    if dict {
        chips.push(Chip::new(DICTIONARY, Look::Outline).icon(Icon::Dictionary));
    }
    if !chips.is_empty() {
        ui.add_space(t.gap * 1.5);
        let pressed = controls::row(ui, chips, 8.0);
        if back && pressed.first().is_some_and(Response::clicked) {
            send(out, Action::ReadPrevious(pane.id));
        }
        if dict && pressed.last().is_some_and(Response::clicked) {
            send(out, Action::UseDictionary(pane.id));
        }
    }
    if let Some(job) = job.filter(|j| j.via() == Via::Ai && !slow) {
        let due = SLOW_SECS - job.waited(pane.now);
        ui.ctx().request_repaint_after_secs(due.max(0.05) as f32);
    }
}

/// How much of the deck is written, once the AI is writing it.
fn written(job: &DeckJob) -> Option<f32> {
    let (done, of) = job.progress().filter(|_| job.doing() == Doing::Writing)?;
    Some(done as f32 / of.max(1) as f32)
}

/// Centred words, wrapped to the column.
fn centred(ui: &mut Ui, text: RichText) {
    ui.add(Label::new(text).wrap().halign(Align::Center));
}

/// A thin track: filled to `done` while cards are written, else a glint
/// that runs along it while the words are found.
fn bar(ui: &mut Ui, done: Option<f32>, t: &Tokens) {
    let (track, _) = ui.allocate_exact_size(vec2(BAR_W, 6.0), Sense::hover());
    let (from, to) = match done {
        Some(done) => (0.0, done.clamp(0.0, 1.0)),
        None => {
            let at = (ui.input(|i| i.time) as f32 / 1.4).fract() * 1.4 - 0.4;
            (at.max(0.0), (at + 0.4).min(1.0))
        }
    };
    let x = |k: f32| track.left() + track.width() * k;
    let right = x(to).max(x(from) + 6.0).min(track.right());
    let lit = Rect::from_x_y_ranges(x(from)..=right, track.y_range());
    let painter = ui.painter();
    painter.rect_filled(track, 3.0, t.ink.gamma_multiply(0.08));
    painter.rect_filled(lit, 3.0, t.accent.gamma_multiply(0.75));
    if done.is_none() && motion::secs(ui.ctx(), 1.0) > 0.0 {
        ui.ctx().request_repaint();
    }
}

/// Why the words for `range` couldn't be made, and the ways on.
pub fn failed(ui: &mut Ui, pane: &Pane<'_>, range: &str, why: &str, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let (mark, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
    ui.painter()
        .circle_filled(mark.center(), 22.0, t.danger.gamma_multiply(0.12));
    let glyph = Rect::from_center_size(mark.center(), Vec2::splat(20.0));
    Icon::Alert.paint(ui, glyph, t.danger);
    ui.add_space(t.gap);
    let title = format!("The words for {range} couldn’t be prepared");
    let font = theme::semibold(20.0);
    centred(ui, RichText::new(title).font(font).color(t.ink));
    ui.allocate_ui(vec2(NOTE_W.min(ui.available_width()), 0.0), |ui| {
        centred(ui, RichText::new(why).size(t.text).color(t.muted));
        let kept = "Nothing is lost: your place is kept.";
        centred(ui, RichText::new(kept).size(t.text_small).color(t.muted));
    });
    ui.add_space(t.gap * 1.5);
    let from = pane.run.gate_batch().map(|b| b.from);
    let offered = pane.dict && pane.run.fallback != from;
    let back = pane.run.back(pane.lock).is_some();
    let mut chips = vec![Chip::new(RETRY, Look::Primary).icon(Icon::Rescan)];
    if offered {
        chips.push(Chip::new(DICTIONARY, Look::Outline).icon(Icon::Dictionary));
    }
    if back {
        chips.push(Chip::new(PREVIOUS, Look::Outline).icon(Icon::ChevronLeft));
    }
    let pressed = controls::row(ui, chips, 8.0);
    let clicked = |i: usize| pressed.get(i).is_some_and(Response::clicked);
    if clicked(0) {
        send(out, Action::Retry(pane.id));
    }
    if offered && clicked(1) {
        send(out, Action::UseDictionary(pane.id));
    }
    if back && clicked(pressed.len().saturating_sub(1)) {
        send(out, Action::ReadPrevious(pane.id));
    }
}
