//! The shared expanded vocabulary card, whatever opened it.

use egui::{Align, Frame, Label, Layout, Margin, RichText, Ui, vec2};
use mareader_engine::space::{self, History};
use mareader_library::{LearnedBy, RecallAnswer};

use super::Detail;
use crate::icons::Icon;
use crate::space::{look, sentence};
use crate::theme::{self, Tokens};
use crate::widgets;

const SPLIT_FROM: f32 = 490.0;

pub(super) fn show(ui: &mut Ui, detail: &Detail, t: &Tokens, height: f32) -> bool {
    ui.set_width(ui.available_width());
    let before = ui.cursor().top();
    let close = header(ui, detail, t);
    ui.add_space(8.0);
    ui.separator();
    let used = ui.cursor().top() - before;
    egui::ScrollArea::vertical()
        .id_salt("word-detail-scroll")
        .max_height((height - used).max(0.0))
        .show(ui, |ui| body(ui, detail, t));
    close
}

fn header(ui: &mut Ui, detail: &Detail, t: &Tokens) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 38.0).max(0.0);
        ui.allocate_ui_with_layout(vec2(width, 24.0), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.add(Icon::Book.image(ui, 15.0).tint(t.muted));
            let books = detail.row.books.join("  ·  ");
            let label = RichText::new(&books).font(theme::medium(t.text_small + 1.0)).color(t.muted);
            ui.add(Label::new(label).truncate().selectable(false))
                .on_hover_text(books);
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = widgets::small_icon_button(ui, Icon::Close, "Close word details").clicked();
        });
    });
    close
}

fn body(ui: &mut Ui, detail: &Detail, t: &Tokens) {
    ui.spacing_mut().item_spacing.y = 7.0;
    ui.add_space(11.0);
    let word = RichText::new(&detail.row.lemma).font(theme::semibold(t.text + 12.0)).color(t.ink);
    ui.add(Label::new(word).wrap().selectable(false));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 7.0;
        if let Some(pos) = &detail.row.pos {
            ui.label(RichText::new(pos).italics().color(t.muted));
        }
        look::level(ui, detail.row.band, t);
        look::tag(ui, space::status_label(detail.row.status), look::status(detail.row.status, t), 6.0);
        let by = match detail.row.by {
            LearnedBy::Proactive => "Card".to_owned(),
            LearnedBy::Reactive if detail.row.tries == 1 => "Recall · 1 try".to_owned(),
            LearnedBy::Reactive => format!("Recall · {} tries", detail.row.tries),
        };
        look::quiet(ui, &by, t.text_small, t);
    });
    look::quiet(ui, &format!("Last met {}", space::date(detail.row.seen)), t.text_small, t);
    ui.add_space(9.0);
    if ui.available_width() >= SPLIT_FROM {
        ui.columns(2, |columns| {
            if let [left, right] = columns {
                core(left, detail, t);
                extra(right, detail, t);
            }
        });
    } else {
        core(ui, detail, t);
        ui.add_space(12.0);
        extra(ui, detail, t);
    }
    recall(ui, detail, t);
    ui.add_space(10.0);
}

fn core(ui: &mut Ui, detail: &Detail, t: &Tokens) {
    look::caption(ui, "Meaning", t);
    let meaning = if detail.row.meaning.is_empty() { "No saved meaning." } else { &detail.row.meaning };
    ui.add(Label::new(RichText::new(meaning).size(t.text + 3.0).color(t.ink)).wrap());
    if !detail.row.context.is_empty() {
        ui.add_space(12.0);
        Frame::new()
            .fill(t.paper)
            .stroke(egui::Stroke::new(1.0, t.line))
            .corner_radius(12)
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                look::caption(ui, "In context", t);
                ui.add_space(3.0);
                ui.add(Label::new(sentence::highlight(&detail.row.context, &detail.row.form, t)).wrap());
            });
    }
}

fn extra(ui: &mut Ui, detail: &Detail, t: &Tokens) {
    let mut synonyms = detail.row.synonyms.clone();
    let mut examples = detail.row.usages.clone();
    let mut definitions = Vec::new();
    if let Some(entries) = &detail.entries {
        for entry in entries {
            if !entry.definition.is_empty() && entry.definition != detail.row.meaning && !definitions.contains(&entry.definition) {
                definitions.push(entry.definition.clone());
            }
            if let Some(ai) = &entry.ai {
                for value in &ai.synonyms { if !synonyms.contains(value) { synonyms.push(value.clone()); } }
                for value in &ai.usages { if !examples.contains(value) { examples.push(value.clone()); } }
            }
        }
    }
    if !synonyms.is_empty() {
        look::caption(ui, "Synonyms", t);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for word in &synonyms { synonym(ui, word, t); }
        });
    }
    if !examples.is_empty() {
        if !synonyms.is_empty() { ui.add_space(10.0); }
        look::caption(ui, "Examples", t);
        for value in &examples {
            ui.add(Label::new(RichText::new(value).font(theme::serif(t.text)).italics().color(t.ink)).wrap());
        }
    }
    if !definitions.is_empty() {
        if !synonyms.is_empty() || !examples.is_empty() { ui.add_space(10.0); }
        look::caption(ui, "Other meanings", t);
        for value in definitions { ui.add(Label::new(value).wrap()); }
    }
    if synonyms.is_empty() && examples.is_empty() && detail.entries.is_some() {
        look::caption(ui, "More details", t);
        look::quiet(ui, "No synonyms or examples available.", t.text_small + 1.0, t);
    }
    if detail.entries.is_none() {
        look::quiet(ui, "Checking dictionaries…", t.text_small, t);
    } else if let Some(why) = &detail.failure {
        look::quiet(ui, &format!("Dictionary lookup: {why}"), t.text_small, t);
    }
}

fn synonym(ui: &mut Ui, word: &str, t: &Tokens) {
    Frame::new()
        .fill(t.accent_soft)
        .corner_radius(7)
        .inner_margin(Margin::symmetric(9, 5))
        .show(ui, |ui| {
            let text = RichText::new(word).font(theme::medium(t.text_small + 1.0)).color(t.accent);
            ui.add(Label::new(text).wrap().selectable(false));
        });
}

fn recall(ui: &mut Ui, detail: &Detail, t: &Tokens) {
    if detail.history.is_empty() && detail.row.by != LearnedBy::Reactive { return; }
    ui.add_space(14.0);
    ui.separator();
    ui.add_space(7.0);
    look::caption(ui, "Your recall", t);
    if detail.history.is_empty() {
        look::quiet(ui, "Answer text wasn't saved for earlier recalls.", t.text_small + 1.0, t);
        return;
    }
    for run in &detail.history {
        let title = session_title(detail, run);
        if !title.is_empty() {
            look::quiet(ui, &title, t.text_small, t);
        }
        for answer in &run.answers { attempt(ui, answer, t); }
        ui.add_space(4.0);
    }
}

pub(super) fn session_title(detail: &Detail, run: &History) -> String {
    let date = space::date(run.when);
    let many_books = detail.row.books.len() > 1;
    let show_date = detail.history.len() > 1 || date != space::date(detail.row.seen);
    match (many_books, show_date) {
        (true, true) => format!("{} · {date}", run.book),
        (true, false) => run.book.clone(),
        (false, true) => date,
        (false, false) => String::new(),
    }
}

fn attempt(ui: &mut Ui, answer: &RecallAnswer, t: &Tokens) {
    let color = match answer.verdict.as_str() {
        "Correct" => look::swatch(t.toast.info).0,
        "Close" => look::swatch(t.toast.warning).0,
        "Wrong" => look::swatch(t.toast.error).0,
        _ => t.muted,
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new(&answer.verdict).font(theme::medium(t.text_small + 1.0)).color(color));
        ui.add(Label::new(RichText::new(&answer.text).color(t.ink)).wrap());
    });
    if !answer.feedback.is_empty() {
        look::quiet(ui, &answer.feedback, t.text_small, t);
    }
}
