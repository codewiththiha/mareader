//! One dictionary entry's rows, shared by the card and the panel.

use egui::text::LayoutJob;
use egui::{Align, FontId, Label, Layout, RichText, TextFormat, Ui, vec2};
use mareader_dict::{Entry, HUB, WordMatch, fold};
use mareader_speech::primary;

use crate::shaped;
use crate::theme::Tokens;

/// The answer reads a step larger than the interface, as on a page.
pub const ANSWER: f32 = 1.15;
/// Burmese and its kin run taller at a size, so read a step smaller.
pub const TALL: f32 = 1.05;

/// The answer's size: tall scripts take the smaller step.
pub fn answer_size(text: &str, t: &Tokens) -> f32 {
    t.text
        * if shaped::needs_shaping(text) {
            TALL
        } else {
            ANSWER
        }
}

/// The languages of an entry's two sides, as `en`; a bridge answers in `to`.
pub fn sides(e: &Entry, to: Option<&str>) -> (String, String) {
    let pack = mareader_dict::PACKS.iter().find(|p| p.id == e.pack);
    let (source, target) = pack.map_or((HUB, HUB), |p| (p.source, p.target));
    let (asked, answer) = match e.flipped {
        true => (target, source),
        false => (source, target),
    };
    let answer = to.filter(|_| e.via.is_some()).unwrap_or(answer);
    (primary(asked), primary(answer))
}

/// The headword an entry stands for when it isn't the word asked.
pub fn near<'a>(e: &'a Entry, asked: &str) -> Option<&'a str> {
    // A bridge's asked side is the English between, not the ask.
    let other = e.via.is_none() && fold(e.asked()) != fold(asked);
    (e.fit != WordMatch::Exact || other).then(|| e.asked())
}

/// "≈ word": the nearest entry stands in for the one asked.
pub fn heading(ui: &mut Ui, word: &str, t: &Tokens) {
    let text = format!("≈ {word}");
    shaped::label(ui, &text, t.text_small, t.muted).on_hover_text("The nearest entry");
}

/// Tags, unless shown elsewhere, and source; the answer, romanization, sense.
pub fn rows(ui: &mut Ui, e: &Entry, t: &Tokens, tags: bool) {
    meta(ui, e, t, tags);
    shaped::label(ui, e.answer(), answer_size(e.answer(), t), t.ink);
    if let Some(r) = e.romanization.as_deref().filter(|r| !r.is_empty()) {
        shaped::label(ui, r, t.text_small, t.muted);
    }
    if let Some(s) = e.sense.as_deref().filter(|s| !s.is_empty()) {
        if shaped::needs_shaping(s) {
            shaped::label(ui, s, t.text_small, t.muted);
        } else {
            ui.label(RichText::new(s).italics().size(t.text_small).color(t.muted));
        }
    }
}

/// Tags left, "via perfume" right; the tags give way, never the bridge.
fn meta(ui: &mut Ui, e: &Entry, t: &Tokens, shown: bool) {
    let tags = if shown { e.tags() } else { String::new() };
    if tags.is_empty() && e.via.is_none() {
        return;
    }
    let width = ui.available_width();
    ui.horizontal(|ui| {
        ui.set_max_width(width);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(via) = &e.via {
                // Up to two thirds of the row; past that it ends in "…".
                let cap = vec2((width * 0.66).max(60.0), 0.0);
                let right = Layout::right_to_left(Align::Center);
                ui.allocate_ui_with_layout(cap, right, |ui| {
                    let label = Label::new(bridge(via, t)).truncate();
                    let tip = format!("Found through the English “{via}”");
                    ui.add(label).on_hover_text(tip);
                });
            }
            if !tags.is_empty() {
                let lead = Layout::left_to_right(Align::Center);
                ui.with_layout(lead, |ui| {
                    let tags = RichText::new(tags).italics().size(t.text_small);
                    ui.add(Label::new(tags.color(t.muted)).truncate());
                });
            }
        });
    });
}

/// "via" quiet, the English word between a shade stronger.
fn bridge(via: &str, t: &Tokens) -> LayoutJob {
    let font = FontId::proportional(t.text_small);
    let mut job = LayoutJob::default();
    let quiet = TextFormat::simple(font.clone(), t.muted);
    job.append("via ", 0.0, quiet);
    job.append(
        via,
        0.0,
        TextFormat::simple(font, t.ink.gamma_multiply(0.8)),
    );
    job
}
