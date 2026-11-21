//! Space's rows under their headings: a list, or cards in a grid.

use egui::{Align, Frame, Label, Layout, Margin, RichText, ScrollArea, Stroke, Ui, vec2};
use mareader_engine::command::Command;
use mareader_engine::space::{self, Action, Row, View};
use mareader_library::{LearnedBy, WordStatus};

use super::look;
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;

const CARD_W: f32 = 236.0;
const NONE_YET: &str = "Study some pages, and the words you meet land here.";
const NONE_MATCH: &str = "Clear a filter, or study some pages to add more.";

/// How the rows are drawn, and which one is open to its sentence.
pub(super) struct Shown<'a> {
    pub cards: bool,
    pub open: &'a mut Option<u128>,
}

/// Every section in a scroll area; an empty page says why.
pub(super) fn show(ui: &mut Ui, view: &View, shown: Shown<'_>, t: &Tokens, out: &mut Commands) {
    if view.rows().next().is_none() {
        let (title, hint) = match view.total {
            0 => ("No words yet", NONE_YET),
            _ => ("No words match", NONE_MATCH),
        };
        widgets::panel_empty(ui, Some(Icon::Levels), title, hint);
        return;
    }
    let Shown { cards, open } = shown;
    let scroll = ScrollArea::vertical()
        .id_salt("space-rows")
        .auto_shrink([false, false]);
    let scrolled = scroll.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        for section in &view.sections {
            if !section.label.is_empty() {
                let n = section.rows.len();
                look::caption(ui, &format!("{} · {n}", section.label), t);
            }
            match cards {
                true => grid(ui, &section.rows, t, out),
                false => list(ui, &section.rows, open, t, out),
            }
        }
    });
    widgets::scroll_fades(ui, &scrolled, t.paper);
}

fn list(ui: &mut Ui, rows: &[Row], open: &mut Option<u128>, t: &Tokens, out: &mut Commands) {
    for row in rows {
        ui.push_id(row.key, |ui| line(ui, row, open, t, out));
    }
}

/// One row: the word and its meaning, its date and actions, then badges.
fn line(ui: &mut Ui, row: &Row, open: &mut Option<u128>, t: &Tokens, out: &mut Commands) {
    let is_open = *open == Some(row.key);
    card_frame(t).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if chevron(ui, is_open, &row.lemma) {
                *open = if is_open { None } else { Some(row.key) };
            }
            actions(ui, row, out);
            look::quiet(ui, &space::day(row.seen), t);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                word(ui, row, t);
                let meaning = RichText::new(&row.meaning).color(t.muted);
                ui.add(Label::new(meaning).truncate());
            });
        });
        ui.horizontal_wrapped(|ui| badges(ui, row, t));
        if is_open {
            sentence(ui, row, t);
        }
    });
}

/// The words as cards, wrapping across the page.
fn grid(ui: &mut Ui, rows: &[Row], t: &Tokens, out: &mut Commands) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        for row in rows {
            ui.push_id(row.key, |ui| card(ui, row, t, out));
        }
    });
}

fn card(ui: &mut Ui, row: &Row, t: &Tokens, out: &mut Commands) {
    card_frame(t).show(ui, |ui| {
        ui.set_width(CARD_W);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.horizontal(|ui| word(ui, row, t));
            let meaning = RichText::new(&row.meaning).color(t.muted);
            ui.add(Label::new(meaning).wrap());
            ui.horizontal_wrapped(|ui| badges(ui, row, t));
            ui.horizontal(|ui| {
                look::quiet(ui, &space::day(row.seen), t);
                let right = Layout::right_to_left(Align::Center);
                ui.with_layout(right, |ui| actions(ui, row, out));
            });
        });
    });
}

fn card_frame(t: &Tokens) -> Frame {
    Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius)
        .inner_margin(Margin::symmetric(12, 8))
}

/// The lemma, its class and its level.
fn word(ui: &mut Ui, row: &Row, t: &Tokens) {
    let lemma = RichText::new(&row.lemma).font(theme::medium(t.text + 2.0));
    ui.add(Label::new(lemma.color(t.ink)).selectable(false));
    if let Some(pos) = &row.pos {
        look::quiet(ui, pos, t);
    }
    look::level(ui, mareader_cefr::label(row.band), t);
}

/// Its status, how it was learned, a first try, and its books.
fn badges(ui: &mut Ui, row: &Row, t: &Tokens) {
    let ink = match row.status {
        WordStatus::Memorized => t.accent,
        WordStatus::Missed => t.danger,
        WordStatus::Learning => t.muted,
    };
    look::tag(ui, space::status_label(row.status), ink, t);
    let learned = match (row.by, row.tries) {
        (LearnedBy::Proactive, _) => "card".to_owned(),
        (LearnedBy::Reactive, 1) => "1 try".to_owned(),
        (LearnedBy::Reactive, n) => format!("{n} tries"),
    };
    look::tag(ui, &learned, t.muted, t);
    if row.first_try {
        look::tag(ui, "first try", t.accent, t);
    }
    look::quiet(ui, &row.books.join(" · "), t);
}

/// The book's sentence, and how many books met the word.
fn sentence(ui: &mut Ui, row: &Row, t: &Tokens) {
    let said = RichText::new(&row.context).font(theme::serif(t.text + 1.0));
    ui.add(Label::new(said.color(t.ink).italics()).wrap());
    let n = row.books.len();
    if n > 1 {
        look::quiet(ui, &format!("Met in {n} books"), t);
    }
}

/// Memorize or un-memorize, and forget: each acts on the word everywhere.
fn actions(ui: &mut Ui, row: &Row, out: &mut Commands) {
    let words = vec![row.word];
    let forget = format!("Forget {}", row.lemma);
    if widgets::small_icon_button(ui, Icon::Remove, &forget).clicked() {
        out.push(Sent::Engine(Command::Space(Action::Forget(words.clone()))));
    }
    let (icon, label, action) = match row.status {
        WordStatus::Memorized => (Icon::Restore, "Un-memorize", Action::Unmemorize(words)),
        _ => (Icon::Check, "Memorize", Action::Memorize(words)),
    };
    let label = format!("{label} {}", row.lemma);
    if widgets::small_icon_button(ui, icon, &label).clicked() {
        out.push(Sent::Engine(Command::Space(action)));
    }
}

/// Opens or closes the row's sentence; true when clicked.
fn chevron(ui: &mut Ui, open: bool, lemma: &str) -> bool {
    let (icon, verb) = match open {
        true => (Icon::ChevronUp, "Hide"),
        false => (Icon::ChevronDown, "Show"),
    };
    let label = format!("{verb} where {lemma} was met");
    widgets::small_icon_button(ui, icon, &label).clicked()
}
