//! The deck's place while it is made, and when it couldn't be.
//!
//! While it is made, blank cards drift over the frost with the stage the
//! deck is at beneath, and a line filled as far as the deck is made, as
//! the corner toast's ring is. Once the AI has taken [`SLOW_SECS`] it says
//! so and offers the dictionary. A failure offers the setup it lacks, or
//! another try, then the dictionary and the pages before the gate.

use egui::{Align, Id, Label, Rect, Response, RichText, Sense, Ui, Vec2, vec2};
use mareader_engine::command::Command;
use mareader_engine::settings::Tab;
use mareader_engine::study_mode::{Action, DeckJob, Doing, Setup};
use mareader_study::mode::{SLOW_SECS, Via};
use mareader_workspace::PaneId;

use super::controls::{self, Chip, Look};
use super::deck::{self, send};
use super::{Pane, making, progress};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};

/// The buttons' labels, for screen readers and tests.
pub const DICTIONARY: &str = "Use the dictionary instead";
pub const RETRY: &str = "Try again";
pub const PREVIOUS: &str = "Read previous pages";
/// The setup a failed deck asks for, when another try can't mend it.
pub const SET_UP_AI: &str = "Set up AI…";
pub const GET_DICTIONARY: &str = "Get a dictionary…";
pub const CHECK_MODEL: &str = "Check the model…";
pub const GET_WORD_DATA: &str = "Get word data…";
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
            match job.progress() {
                _ if job.queued() => format!("Waiting for {by}, busy with other work…{skipped}"),
                Some((done, of)) if done > 0 => {
                    format!("Writing meanings in context with {by}: {done} of {of}…{skipped}")
                }
                _ => format!("Writing meanings in context with {by}…{skipped}"),
            }
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
    let made = job.map_or(0.0, DeckJob::made);
    bar(ui, Id::new(("study-made", pane.id)), made, &t);
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

/// Centred words, wrapped to the column.
fn centred(ui: &mut Ui, text: RichText) {
    ui.add(Label::new(text).wrap().halign(Align::Center));
}

/// A thin track filled to `made`, easing to each step, with a soft pulse
/// at its head while the work goes on.
fn bar(ui: &mut Ui, id: Id, made: f32, t: &Tokens) {
    let (track, _) = ui.allocate_exact_size(vec2(BAR_W, 6.0), Sense::hover());
    let made = progress::eased(ui.ctx(), id, made);
    let head = track.left() + (track.width() * made).max(track.height());
    let lit = Rect::from_x_y_ranges(track.left()..=head, track.y_range());
    let painter = ui.painter();
    painter.rect_filled(track, 3.0, t.ink.gamma_multiply(0.08));
    painter.rect_filled(lit, 3.0, t.accent.gamma_multiply(0.75));
    let pulse = progress::pulse(ui.ctx());
    let glow = t.accent.gamma_multiply(0.25 + 0.5 * pulse);
    let tip = egui::pos2(head - track.height() / 2.0, track.center().y);
    painter.circle_filled(tip, track.height() * 0.8, glow);
}

/// Why the words for `range` couldn't be made, and the ways on: what to
/// set up first, when another try alone can't mend it.
pub fn failed(
    ui: &mut Ui,
    pane: &Pane<'_>,
    range: &str,
    why: (&str, Option<Setup>),
    out: &mut Commands,
) {
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
        centred(ui, RichText::new(why.0).size(t.text).color(t.muted));
        let kept = "Nothing is lost: your place is kept.";
        centred(ui, RichText::new(kept).size(t.text_small).color(t.muted));
    });
    ui.add_space(t.gap * 1.5);
    let (chips, ways): (Vec<_>, Vec<_>) = ways_on(pane, why.1).into_iter().unzip();
    let pressed = controls::row(ui, chips, 8.0);
    for (way, button) in ways.into_iter().zip(&pressed) {
        if button.clicked() {
            way.send(pane.id, out);
        }
    }
}

/// A way on from a failed deck.
#[derive(Clone, Copy)]
enum Way {
    Settings(Tab),
    Retry,
    Dictionary,
    Previous,
}

impl Way {
    fn send(self, pane: PaneId, out: &mut Commands) {
        match self {
            Self::Settings(tab) => out.push(Sent::Engine(Command::OpenSettings(tab))),
            Self::Retry => send(out, Action::Retry(pane)),
            Self::Dictionary => send(out, Action::UseDictionary(pane)),
            Self::Previous => send(out, Action::ReadPrevious(pane)),
        }
    }
}

/// The failed deck's buttons: the setup it wants leads, then another try
/// where one may help, the dictionary and the pages before.
fn ways_on(pane: &Pane<'_>, setup: Option<Setup>) -> Vec<(Chip<'static>, Way)> {
    let chip = |label, look, icon| Chip::new(label, look).icon(icon);
    let (lead, side) = (Look::Primary, Look::Outline);
    let mut ways = match setup {
        Some(Setup::Writer) => vec![(chip(SET_UP_AI, lead, Icon::Ai), Way::Settings(Tab::Ai))],
        Some(Setup::Model) => {
            let to = Way::Settings(Tab::Models);
            vec![(chip(CHECK_MODEL, lead, Icon::Key), to)]
        }
        Some(Setup::WordData) => {
            let to = Way::Settings(Tab::Vocabulary);
            vec![(chip(GET_WORD_DATA, lead, Icon::Levels), to)]
        }
        None => vec![(chip(RETRY, lead, Icon::Rescan), Way::Retry)],
    };
    match setup {
        // Setting a writer up makes the deck again by itself.
        Some(Setup::Writer) if !pane.dict => {
            let to = Way::Settings(Tab::Dictionary);
            ways.push((chip(GET_DICTIONARY, side, Icon::Dictionary), to));
        }
        Some(Setup::Model | Setup::WordData) => {
            ways.push((chip(RETRY, side, Icon::Rescan), Way::Retry));
        }
        _ => {}
    }
    let from = pane.run.gate_batch().map(|b| b.from);
    if pane.dict && pane.run.fallback != from {
        ways.push((chip(DICTIONARY, side, Icon::Dictionary), Way::Dictionary));
    }
    if pane.run.back(pane.lock).is_some() {
        ways.push((chip(PREVIOUS, side, Icon::ChevronLeft), Way::Previous));
    }
    ways
}
