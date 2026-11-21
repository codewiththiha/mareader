//! Reactive mode's recall popover: what a shaded word means here, in the
//! reader's own words, judged by the AI.
//!
//! The engine holds the recall (`mareader_engine::study_mode::Recalling`);
//! this card draws its phase beside the word and sends what the reader
//! asks. Only the answer being typed lives here.

use std::ops::Range;

use egui::{Color32, Context, Id, Key, Margin, Rect, RichText, Sense, Ui, Vec2};
use mareader_engine::study_mode::{Action, Ask, Recalling};
use mareader_study::mode::{MAX_TRIES, Outcome, Phase, Verdict};
use mareader_workspace::PaneId;

use super::deck::send;
use crate::bubble::{Bubble, Side};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::Commands;
use crate::widgets;

/// The buttons' labels, for screen readers and tests.
pub const CHECK: &str = "Check";
pub const UNSURE: &str = "I don’t know";
pub const SHOW: &str = "Show answer";
pub const AGAIN: &str = "Try again";
pub const KEEP: &str = "Keep practising";
pub const MEMORIZED: &str = "Memorized";
pub const GOT_IT: &str = "Got it";
pub const RETRY: &str = "Retry";
pub const CANCEL: &str = "Cancel";
pub const CLOSE: &str = "Close recall";
/// The answer box's placeholder.
pub const HINT: &str = "In your own words, in any language…";
/// The card's width inside its margins.
const WIDTH: f32 = 336.0;

/// The answer being typed, for the recall it was typed in.
#[derive(Default)]
pub struct Draft {
    text: String,
    /// The word it belongs to; a new recall starts empty.
    word: Option<(PaneId, usize, Range<usize>)>,
    /// The tries used when the box last took focus; each try takes it once.
    focused: Option<u8>,
}

impl Draft {
    fn follow(&mut self, recalling: &Recalling) {
        let word = &recalling.word;
        let now = Some((recalling.pane, word.page, word.chars.clone()));
        if self.word != now {
            self.word = now;
            self.text.clear();
            self.focused = None;
        }
    }
}

/// The popover for `recalling`, pointing at its word at `at` within `room`.
pub fn show(
    ctx: &Context,
    recalling: &Recalling,
    (at, room): (Rect, Rect),
    draft: &mut Draft,
    out: &mut Commands,
) {
    draft.follow(recalling);
    let t = Tokens::get(ctx);
    let pane = recalling.pane;
    let bubble = Bubble {
        id: Id::new(("study-recall", pane)),
        at,
        room,
        width: WIDTH,
        margin: Margin::same(12),
        snug: false,
        prefer: Side::Below,
    };
    bubble.show(ctx, &t, |ui, _| {
        header(ui, recalling, out);
        ui.add_space(t.gap);
        body(ui, recalling, draft, out);
    });
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
        send(out, recall(pane, Ask::Close));
    }
}

fn recall(pane: PaneId, ask: Ask) -> Action {
    Action::Recall { pane, ask }
}

/// The tag, the word's band, the tries used and the close button.
fn header(ui: &mut Ui, recalling: &Recalling, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let tag = RichText::new("RECALL").size(t.text_small).color(t.accent).strong();
        ui.label(tag);
        let band = mareader_cefr::label(recalling.word.band);
        if !band.is_empty() {
            ui.label(RichText::new(band).size(t.text_small).color(t.muted));
        }
        tries(ui, recalling.recall.attempts, &t);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::small_icon_button(ui, Icon::Close, CLOSE).clicked() {
                send(out, recall(recalling.pane, Ask::Close));
            }
        });
    });
}

/// One dot per try, filled once used.
fn tries(ui: &mut Ui, used: u8, t: &Tokens) {
    let size = Vec2::new(f32::from(MAX_TRIES) * 10.0, 10.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    for i in 0..MAX_TRIES {
        let x = rect.left() + 5.0 + f32::from(i) * 10.0;
        let centre = egui::pos2(x, rect.center().y);
        let fill = if i < used { t.accent } else { Color32::TRANSPARENT };
        ui.painter().circle(centre, 3.0, fill, egui::Stroke::new(1.0, t.muted));
    }
    response.on_hover_text(format!("{used} of {MAX_TRIES} attempts used"));
}

fn body(ui: &mut Ui, recalling: &Recalling, draft: &mut Draft, out: &mut Commands) {
    match recalling.recall.phase {
        Phase::Input => input(ui, recalling, draft, out),
        Phase::Judging => judging(ui, recalling),
        Phase::Result => result(ui, recalling, out),
        Phase::Ask => ask(ui, recalling, out),
        Phase::Revealed => revealed(ui, recalling, out),
        Phase::Failed => failed(ui, recalling, out),
    }
}

/// The question and the answer box; Enter checks.
fn input(ui: &mut Ui, recalling: &Recalling, draft: &mut Draft, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let pane = recalling.pane;
    let asked = format!("What does “{}” mean here?", recalling.word.form);
    ui.label(RichText::new(asked).font(theme::medium(t.text)));
    let sentence = RichText::new(&recalling.word.sentence).size(t.text_small).color(t.muted);
    ui.label(sentence.italics());
    let field = widgets::input_multiline(&mut draft.text)
        .id(Id::new(("study-recall-field", pane)))
        .hint(HINT)
        .rows(2)
        .return_key(None)
        .show(ui)
        .response;
    let tries = recalling.recall.attempts;
    if draft.focused != Some(tries) {
        // A new try starts empty; the first keeps nothing to clear.
        if draft.focused.replace(tries).is_some() {
            draft.text.clear();
        }
        field.request_focus();
    }
    let typing = field.has_focus() || field.lost_focus();
    let enter = typing && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
    let mut sent = enter;
    ui.horizontal(|ui| {
        if ui.button(UNSURE).clicked() {
            send(out, recall(pane, Ask::Reveal));
        }
        let ready = !draft.text.trim().is_empty();
        let check = widgets::primary_button(ui, None, CHECK, t.accent, &t);
        sent |= ui.add_enabled(ready, check).clicked();
    });
    if sent && !draft.text.trim().is_empty() {
        send(out, recall(pane, Ask::Answer(draft.text.clone())));
    }
}

fn judging(ui: &mut Ui, recalling: &Recalling) {
    let t = Tokens::get(ui.ctx());
    let by = Some(recalling.by.as_str()).filter(|b| !b.is_empty());
    let said = format!("Checking with {}…", by.unwrap_or("the AI"));
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(12.0));
        ui.label(RichText::new(said).size(t.text).color(t.muted));
    });
}

/// A close or wrong verdict, the hint after the first miss, and the ways on.
fn result(ui: &mut Ui, recalling: &Recalling, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let pane = recalling.pane;
    let (title, colour) = match recalling.recall.verdict {
        Some(Verdict::Close) => ("Almost", t.accent),
        _ => ("Not quite", t.danger),
    };
    ui.label(RichText::new(title).font(theme::medium(t.text)).color(colour));
    feedback(ui, &recalling.recall.feedback, &t);
    if let Some(hint) = recalling.hint() {
        ui.label(RichText::new(hint).size(t.text_small).color(t.ink));
    }
    ui.add_space(t.gap);
    ui.horizontal(|ui| {
        if ui.button(SHOW).clicked() {
            send(out, recall(pane, Ask::Reveal));
        }
        if recalling.recall.can_try() {
            let again = widgets::primary_button(ui, None, AGAIN, t.accent, &t);
            if ui.add(again).clicked() {
                send(out, recall(pane, Ask::Again));
            }
        }
    });
}

/// Correct: whether to add the word to memorized.
fn ask(ui: &mut Ui, recalling: &Recalling, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let pane = recalling.pane;
    let first = recalling.recall.attempts == 1;
    let title = if first { "Correct, first try" } else { "Correct" };
    ui.label(RichText::new(title).font(theme::medium(t.text)).color(t.accent));
    feedback(ui, &recalling.recall.feedback, &t);
    ui.add_space(t.gap);
    let lemma = &recalling.word.key.lemma;
    let add = format!("Add “{lemma}” to memorized?");
    ui.label(RichText::new(add).font(theme::medium(t.text)));
    let why = "It stops being shaded and won’t come back in flash cards, in any book. \
You can undo this in Space.";
    ui.label(RichText::new(why).size(t.text_small).color(t.muted));
    ui.add_space(t.gap);
    ui.horizontal(|ui| {
        if ui.button(KEEP).clicked() {
            send(out, recall(pane, Ask::Finish(Outcome::Learning)));
        }
        let memorized = widgets::primary_button(ui, Some(Icon::Check), MEMORIZED, t.accent, &t);
        if ui.add(memorized).clicked() {
            send(out, recall(pane, Ask::Finish(Outcome::Memorized)));
        }
    });
}

/// The answer: the meaning and synonyms, once the AI has written them.
fn revealed(ui: &mut Ui, recalling: &Recalling, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new("Meaning").size(t.text_small).color(t.muted));
    match &recalling.back {
        Some(back) => {
            ui.label(RichText::new(&back.meaning).size(t.text).color(t.ink));
            if !back.synonyms.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for synonym in &back.synonyms {
                        let chip = RichText::new(synonym).size(t.text_small);
                        ui.add(egui::Button::new(chip).sense(Sense::hover()));
                    }
                });
            }
        }
        None if recalling.writing_back() => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(RichText::new("Writing the meaning…").color(t.muted));
            });
        }
        None => {
            let none = "The meaning couldn’t be written. Look it up after.";
            ui.label(RichText::new(none).size(t.text_small).color(t.muted));
        }
    }
    let saved = "Saved as missed. It will come back in Space.";
    ui.label(RichText::new(saved).size(t.text_small).color(t.muted));
    ui.add_space(t.gap);
    let done = widgets::primary_button(ui, None, GOT_IT, t.accent, &t);
    if ui.add(done).clicked() {
        send(out, recall(recalling.pane, Ask::Finish(Outcome::Missed)));
    }
}

/// The judge failed: why, and the answer kept to send again.
fn failed(ui: &mut Ui, recalling: &Recalling, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let pane = recalling.pane;
    let why = recalling.failure.as_deref().unwrap_or_default();
    ui.label(RichText::new(why).size(t.text_small).color(t.danger));
    let kept = "Your answer is kept. Nothing was counted as an attempt.";
    ui.label(RichText::new(kept).size(t.text_small).color(t.muted));
    ui.add_space(t.gap);
    ui.horizontal(|ui| {
        if ui.button(CANCEL).clicked() {
            send(out, recall(pane, Ask::Close));
        }
        let retry = widgets::primary_button(ui, Some(Icon::Rescan), RETRY, t.accent, &t);
        if ui.add(retry).clicked() {
            send(out, recall(pane, Ask::Again));
            send(out, recall(pane, Ask::Answer(recalling.answer.clone())));
        }
    });
}

fn feedback(ui: &mut Ui, said: &str, t: &Tokens) {
    if !said.is_empty() {
        ui.label(RichText::new(said).size(t.text).color(t.ink));
    }
}
