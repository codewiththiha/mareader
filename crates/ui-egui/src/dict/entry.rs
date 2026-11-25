//! One dictionary entry's rows, shared by the card and the panel.

use egui::text::LayoutJob;
use egui::{Align, FontId, Label, Layout, RichText, TextFormat, Ui, vec2};
use mareader_dict::{Entry, HUB, WordMatch, fold};
use mareader_speech::primary;
use mareader_study::Pos;

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

/// An entry's two sides by dictionary code, the asked one first; none for
/// a row of no known pack and no AI meaning.
fn codes(e: &Entry) -> Option<(&str, &str)> {
    let pack = mareader_dict::PACKS.iter().find(|p| p.id == e.pack);
    // An AI row answers in the language its meaning was written in.
    let ai = e.ai.as_ref().map(|s| (HUB, s.lang.as_str()));
    let (source, target) = ai.or(pack.map(|p| (p.source, p.target)))?;
    Some(match e.flipped {
        true => (target, source),
        false => (source, target),
    })
}

/// The languages of an entry's two sides, as `en`; a bridge answers in `to`.
pub fn sides(e: &Entry, to: Option<&str>) -> (String, String) {
    let (asked, answer) = codes(e).unwrap_or((HUB, HUB));
    let answer = to.filter(|_| e.via.is_some()).unwrap_or(answer);
    (primary(asked), primary(answer))
}

/// "English → Burmese", from an entry's pack or its AI meaning.
pub fn way(e: &Entry) -> Option<String> {
    let (from, to) = codes(e)?;
    let name = mareader_dict::language;
    Some(format!("{} → {}", name(from), name(to)))
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
    if let Some(s) = &e.ai {
        crate::ai_said::show(ui, (&s.meaning, &s.synonyms, &s.usages), t);
        return;
    }
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

/// A row's part of speech as the one class rule reads it: "verb · noun".
pub fn tags(e: &Entry) -> String {
    Pos::spell(e.pos.as_deref().unwrap_or_default())
}

/// Tags left, "via perfume" right; the tags give way, never the bridge.
fn meta(ui: &mut Ui, e: &Entry, t: &Tokens, shown: bool) {
    let tags = if shown { tags(e) } else { String::new() };
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

/// "AI", for a row an AI wrote and this machine keeps.
pub fn ai_badge(ui: &mut Ui, model: &str, t: &Tokens) {
    let text = RichText::new("AI").size(t.text_small).strong();
    let by = if model.is_empty() { "an AI" } else { model };
    let tip = format!("Written by {by}; kept in this computer's AI dictionary");
    ui.add(Label::new(text.color(t.accent))).on_hover_text(tip);
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

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_dict::{AI_PACK, Saved};

    fn row(pack: &'static str, flipped: bool, ai: Option<&str>) -> Entry {
        let said = |lang: &str| Saved {
            lang: lang.to_owned(),
            ..Saved::default()
        };
        Entry {
            word: "light".into(),
            pos: None,
            definition: String::new(),
            romanization: None,
            sense: None,
            pack,
            via: None,
            fit: WordMatch::Exact,
            flipped,
            ai: ai.map(said).map(Box::new),
        }
    }

    #[test]
    fn the_way_names_the_codes_sides_speak() {
        let back = row("jmdict-en-jp", true, None);
        assert_eq!(way(&back).as_deref(), Some("Japanese → English"));
        assert_eq!(sides(&back, None), ("ja".to_owned(), "en".to_owned()));
        let ai = row(AI_PACK, false, Some("ru"));
        assert_eq!(way(&ai).as_deref(), Some("English → Russian"));
        assert_eq!(sides(&ai, None).1, "ru");
        let stray = row("gone", false, None);
        assert_eq!(way(&stray), None);
        assert_eq!(sides(&stray, None), ("en".to_owned(), "en".to_owned()));
    }
}
