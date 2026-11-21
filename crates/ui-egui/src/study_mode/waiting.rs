//! The deck's place while it is made, and when it couldn't be.
//!
//! The waiting card names the stage the deck is at; once the AI has taken
//! [`SLOW_SECS`] it says so and offers the dictionary. The error card offers
//! another try, the dictionary and the pages before the gate.

use egui::{RichText, Sense, Ui, Vec2};
use mareader_engine::study_mode::{Action, DeckJob, Doing};
use mareader_study::mode::{SLOW_SECS, Via};

use super::Pane;
use super::deck::send;
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::Commands;
use crate::widgets;

/// The buttons' labels, for screen readers and tests.
pub const DICTIONARY: &str = "Use the dictionary instead";
pub const RETRY: &str = "Try again";
pub const PREVIOUS: &str = "Read previous pages";
/// What the waiting card says once the AI is slow.
pub const SLOW: &str = "This is taking longer than usual. You can read the previous pages \
while it finishes, and the deck opens as soon as it's ready.";
const USUAL: &str = "Words are prepared ahead while you read, so you rarely wait. \
You can scroll back in the meantime.";

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

/// The waiting card for `range`, with what the deck is doing.
pub fn waiting(
    ui: &mut Ui,
    pane: &Pane<'_>,
    range: &str,
    job: Option<&DeckJob>,
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let title = format!("Preparing the words for {range}");
    ui.label(RichText::new(title).font(theme::medium(16.0)));
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(12.0));
        ui.label(RichText::new(stage(range, job)).size(t.text).color(t.muted));
    });
    ui.add_space(t.gap);
    let slow = job.is_some_and(|j| slow(j, pane.now));
    let note = if slow { SLOW } else { USUAL };
    ui.label(RichText::new(note).size(t.text_small).color(t.muted));
    ui.add_space(t.gap);
    ui.horizontal(|ui| {
        if pane.run.back(false).is_some() && ui.button(PREVIOUS).clicked() {
            send(out, Action::ReadPrevious(pane.id));
        }
        if slow && pane.dict && ui.button(DICTIONARY).clicked() {
            send(out, Action::UseDictionary(pane.id));
        }
    });
    if let Some(job) = job.filter(|j| j.via() == Via::Ai && !slow) {
        let due = SLOW_SECS - job.waited(pane.now);
        ui.ctx().request_repaint_after_secs(due.max(0.05) as f32);
    }
}

/// The error card for `range`: why, and the ways on.
pub fn failed(ui: &mut Ui, pane: &Pane<'_>, range: &str, why: &str, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let (mark, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
        Icon::Alert
            .image(ui, 18.0)
            .tint(t.danger)
            .paint_at(ui, mark);
        let title = format!("The words for {range} couldn’t be prepared");
        ui.label(RichText::new(title).font(theme::medium(t.text)));
    });
    ui.label(RichText::new(why).size(t.text_small).color(t.muted));
    let kept = "Nothing is lost: your place is kept.";
    ui.label(RichText::new(kept).size(t.text_small).color(t.muted));
    ui.add_space(t.gap);
    let from = pane.run.gate_batch().map(|b| b.from);
    let offered = pane.dict && pane.run.fallback != from;
    ui.horizontal(|ui| {
        let again = widgets::primary_button(ui, Some(Icon::Rescan), RETRY, t.accent, &t);
        if ui.add(again).clicked() {
            send(out, Action::Retry(pane.id));
        }
        if offered && ui.button(DICTIONARY).clicked() {
            send(out, Action::UseDictionary(pane.id));
        }
        if pane.run.back(false).is_some() && ui.button(PREVIOUS).clicked() {
            send(out, Action::ReadPrevious(pane.id));
        }
    });
}
