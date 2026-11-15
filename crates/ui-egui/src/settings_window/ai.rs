//! The AI tab: what the AI does, and which saved model does each.

use egui::{Align, FontId, Layout, Sense, Ui, pos2, vec2};
use mareader_assist::{Board, Job, Load};
use mareader_settings::{
    AiSettings, GlossDensity, GlossSettings, HighlightSettings, LANGUAGES, ModelPreset,
    OrganizeTask, PageRun, PageStart, PageTask, Pen, PresetId, Provider, SummaryStyle,
};

use super::highlights::{palette, preview};
use super::{ROW_H, choices, divider, group, note, row, section};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::settings::Tab;

const PICK_W: f32 = 240.0;
const DICTIONARY: &str = "AI Dictionary";
const BOOKS_LANGUAGE: &str = "The book's language";
const SAME_AS_DICTIONARY: &str = "Same as AI Dictionary";
const PICK_LATER: &str = "Pick when translating";
/// Books per request Apple's small context holds.
const APPLE_BATCH: u16 = 20;

/// The highlight settings and word card the AI Dictionary draws with.
pub type Looks<'a> = (&'a mut HighlightSettings, &'a mut GlossSettings);

pub fn tab(
    ui: &mut Ui,
    ai: &mut AiSettings,
    looks: Looks<'_>,
    board: Option<&Board>,
    out: &mut Commands,
) {
    probe(board, out);
    if ai.presets.is_empty() {
        no_models(ui, out);
    }
    dictionary(ui, ai, looks, board);
    library(ui, ai);
    pages(ui, ai, board);
}

/// Asks once whether Apple Intelligence is on this Mac.
pub(super) fn probe(board: Option<&Board>, out: &mut Commands) {
    if cfg!(target_os = "macos") && board.is_some_and(|b| b.apple == Load::Idle) {
        out.push(Sent::Engine(Command::Ai(Job::Probe)));
    }
}

/// What a task with no model of its own falls back to.
fn fallback(ai: &AiSettings) -> &'static str {
    if ai.tasks.explain.preset.is_some() {
        SAME_AS_DICTIONARY
    } else {
        "None"
    }
}

/// With no model saved yet: one step to the models tab.
fn no_models(ui: &mut Ui, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    section(ui, "Get started");
    group(ui, |ui| {
        let hint = "Every task below asks a model you save there";
        row(ui, "No AI models yet", hint, |ui| {
            let button = widgets::primary_button(ui, Some(Icon::Chip), "Add a model", t.accent, &t);
            if ui.add(button).clicked() {
                out.push(Sent::Engine(Command::OpenSettings(Tab::Models)));
            }
        });
    });
}

/// The AI Dictionary: its model, how its words look, its language.
fn dictionary(ui: &mut Ui, ai: &mut AiSettings, (h, gloss): Looks<'_>, board: Option<&Board>) {
    // The row below carries the name; the heading says what it's for.
    section(ui, "Words");
    let apple = ai
        .explain_preset()
        .is_some_and(|p| p.provider == Provider::Apple);
    let state = board.and_then(|b| b.apple.ready()).filter(|_| apple);
    let speaks = |tag: &str| state.and_then(|s| s.speaks(tag));
    let id = egui::Id::new("ai-dictionary-look");
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    group(ui, |ui| {
        let look = (h.ai.pen, h.ai.color, h.opacity);
        dictionary_row(ui, &mut open, look, |ui| {
            let chosen = &mut ai.tasks.explain.preset;
            preset_picker(ui, "ai-explain-model", &ai.presets, chosen, "None");
        });
        if open {
            look_rows(ui, h, gloss);
        }
        divider(ui);
        let tip = "The meaning's language; examples stay in the book's";
        row(ui, "Meaning in", tip, |ui| {
            let meaning = &mut ai.tasks.explain.language;
            language_picker(
                ui,
                ("ai-meaning-language", BOOKS_LANGUAGE),
                meaning,
                &speaks,
            );
        });
    });
    ui.data_mut(|d| d.insert_temp(id, open));
    if open {
        let kept = "Highlights can't take this look, so none passes for an AI mark.";
        note(ui, kept);
    }
    let chosen = ai.tasks.explain.language.as_deref();
    let name = chosen.and_then(mareader_settings::language_name);
    if let (Some(tag), Some(name)) = (chosen, name)
        && speaks(tag) == Some(false)
    {
        note(
            ui,
            &format!("Apple Intelligence doesn't write {name}. Pick a language it lists."),
        );
    }
}

/// The model row; its name, a chevron and the mark fold the look open.
fn dictionary_row(
    ui: &mut Ui,
    open: &mut bool,
    (pen, color, opacity): (Pen, mareader_appearance::Rgb, f32),
    picker: impl FnOnce(&mut Ui),
) {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), ROW_H);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_height(ROW_H);
        let font = FontId::proportional(t.text);
        let text = ui
            .painter()
            .layout_no_wrap(DICTIONARY.into(), font.clone(), t.ink);
        let wide = text.size().x + 6.0 + 16.0 + 8.0 + 34.0;
        let (rect, r) = ui.allocate_exact_size(vec2(wide, ROW_H), Sense::click());
        r.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, *open, DICTIONARY)
        });
        let tip = if *open {
            "Hide its look"
        } else {
            "How its words are marked"
        };
        widgets::tip(&r, tip, None);
        if r.clicked() {
            *open = !*open;
        }
        let ink = if r.hovered() { t.accent } else { t.ink };
        let left = pos2(rect.left(), rect.center().y);
        let painter = ui.painter();
        painter.text(left, egui::Align2::LEFT_CENTER, DICTIONARY, font, ink);
        let chevron = if *open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        let at = pos2(rect.left() + text.size().x + 14.0, rect.center().y);
        let mark = egui::Rect::from_center_size(at, vec2(16.0, 16.0));
        let muted = if r.hovered() { t.accent } else { t.muted };
        chevron.image(ui, 16.0).tint(muted).paint_at(ui, mark);
        // The mark itself, as a page draws it, says what opens.
        let chip = egui::Rect::from_min_size(pos2(rect.right() - 34.0, rect.top()), size);
        ui.scope_builder(egui::UiBuilder::new().max_rect(chip), |ui| {
            ui.set_min_height(ROW_H);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                preview(ui, pen, color, opacity);
            });
        });
        widgets::labelled(ui, r.id, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), picker);
        });
    });
}

/// The AI's pen, colour and card room; highlights then can't take them.
fn look_rows(ui: &mut Ui, h: &mut HighlightSettings, gloss: &mut GlossSettings) {
    let t = Tokens::get(ui.ctx());
    let top = ui.cursor().top();
    let mut ai = h.ai;
    divider(ui);
    row(ui, "Mark", "How its words are marked on the page", |ui| {
        choices(ui, &mut ai.pen, &Pen::ALL, Pen::label);
    });
    divider(ui);
    let taken = h.color(ai.pen);
    row(ui, "Colour", "A pen's own colour can't be used", |ui| {
        let kept = "This pen's own colour for highlights";
        palette(ui, ("AI", kept), &mut ai.color, |c| c != taken);
    });
    // A new pen may meet that pen's own colour; settings move the pen.
    h.ai = ai;
    divider(ui);
    row(ui, "Card spacing", "How roomy the word card is", |ui| {
        let all = [GlossDensity::Compact, GlossDensity::Comfortable];
        choices(ui, &mut gloss.density, &all, GlossDensity::label);
    });
    // A thread down the fold ties its rows to the name above.
    let x = ui.min_rect().left() - 8.0;
    let span = egui::Rangef::new(top + 8.0, ui.cursor().top() - 8.0);
    let stroke = egui::Stroke::new(2.0, t.accent.gamma_multiply(0.5));
    ui.painter().vline(x, span, stroke);
}

/// Sorting books onto shelves: its model and batch size.
fn library(ui: &mut Ui, ai: &mut AiSettings) {
    section(ui, "Library");
    let same = fallback(ai);
    group(ui, |ui| {
        let tip = "The model that sorts imported books on no shelf onto shelves";
        row(ui, "Organize library", tip, |ui| {
            let organize = &mut ai.tasks.organize.preset;
            preset_picker(ui, "ai-organize-model", &ai.presets, organize, same);
        });
        divider(ui);
        let tip = "Books each request reads: more is quicker, fewer suits small models";
        row(ui, "Books per request", tip, |ui| {
            let task = &mut ai.tasks.organize;
            let mut n = f32::from(task.batch);
            let lo = f32::from(OrganizeTask::BATCH_MIN);
            let hi = f32::from(OrganizeTask::BATCH_MAX);
            widgets::stepper(ui, &mut n, lo..=hi, 10.0, |v| format!("{v:.0}"), "books");
            let n = n.round().clamp(lo, hi) as u16;
            task.batch = n;
        });
    });
    let organizer = ai.organize_preset().map(|p| p.provider);
    let big = ai.tasks.organize.batch > APPLE_BATCH;
    if organizer == Some(Provider::Apple) && big {
        note(
            ui,
            &format!(
                "Apple Intelligence reads little at once; \
                 {APPLE_BATCH} books per request or fewer suits it."
            ),
        );
    }
}

/// Summarize, translate and simplify: models, and how pages go on.
fn pages(ui: &mut Ui, ai: &mut AiSettings, board: Option<&Board>) {
    section(ui, "Pages");
    let same = fallback(ai);
    let apple = ai
        .page_preset(PageTask::Translate)
        .is_some_and(|p| p.provider == Provider::Apple);
    let state = board.and_then(|b| b.apple.ready()).filter(|_| apple);
    let speaks = |tag: &str| state.and_then(|s| s.speaks(tag));
    group(ui, |ui| {
        let tip = "The model that summarizes a page";
        row(ui, "Summarize", tip, |ui| {
            let chosen = &mut ai.tasks.summarize.preset;
            preset_picker(ui, "ai-summarize-model", &ai.presets, chosen, same);
        });
        divider(ui);
        row(ui, "Summary style", "Paragraphs, or a short list", |ui| {
            let style = &mut ai.tasks.summarize.style;
            let name = |s: SummaryStyle| match s {
                SummaryStyle::Paragraphs => "Paragraphs",
                SummaryStyle::Points => "Key points",
            };
            widgets::combo("ai-summary-style")
                .width(PICK_W)
                .selected_text(name(*style))
                .show_ui(ui, |ui| {
                    for s in [SummaryStyle::Paragraphs, SummaryStyle::Points] {
                        ui.selectable_value(style, s, name(s));
                    }
                });
        });
        divider(ui);
        row(ui, "Translate", "The model that translates a page", |ui| {
            let chosen = &mut ai.tasks.translate.preset;
            preset_picker(ui, "ai-translate-model", &ai.presets, chosen, same);
        });
        divider(ui);
        let tip = "The page menu offers others too";
        row(ui, "Translate to", tip, |ui| {
            let task = &mut ai.tasks.translate;
            let mut chosen = task.language.clone();
            let id = ("ai-translate-language", PICK_LATER);
            language_picker(ui, id, &mut chosen, &speaks);
            match chosen {
                Some(tag) if task.language.as_deref() != Some(&tag) => task.pick(&tag),
                None => task.language = None,
                Some(_) => {}
            }
        });
        divider(ui);
        let tip = "What choosing Translate in the page menu does";
        row(ui, "Translating", tip, |ui| {
            start_picker(ui, "ai-translate-start", &mut ai.tasks.translate.start);
        });
        divider(ui);
        let tip = "The model that rewrites a page in plain English";
        row(ui, "Simplify", tip, |ui| {
            let chosen = &mut ai.tasks.simplify.preset;
            preset_picker(ui, "ai-simplify-model", &ai.presets, chosen, same);
        });
        divider(ui);
        let tip = "What choosing Simplify in the page menu does";
        row(ui, "Simplifying", tip, |ui| {
            start_picker(ui, "ai-simplify-start", &mut ai.tasks.simplify.start);
        });
    });
    section(ui, "More than one page");
    group(ui, |ui| {
        let tip = "Continuous mode: time on a page, its result shown, before more are asked";
        row(ui, "Time on a page", tip, |ui| {
            let run = &mut ai.tasks.pages;
            let mut n = f32::from(run.dwell);
            let (lo, hi) = (f32::from(PageRun::DWELL_MIN), f32::from(PageRun::DWELL_MAX));
            widgets::stepper(ui, &mut n, lo..=hi, 5.0, |v| format!("{v:.0} s"), "seconds");
            run.dwell = n.round().clamp(lo, hi) as u16;
        });
        divider(ui);
        let tip = "Continuous mode: pages written together, at most";
        row(ui, "Pages at once", tip, |ui| {
            let run = &mut ai.tasks.pages;
            let mut n = f32::from(run.at_once);
            let hi = f32::from(PageRun::AT_ONCE_MAX);
            widgets::stepper(ui, &mut n, 1.0..=hi, 1.0, |v| format!("{v:.0}"), "pages");
            run.at_once = n.round().clamp(1.0, hi) as u8;
        });
        divider(ui);
        let tip = "Pages picked in the thumbnails: how many go in one request";
        row(ui, "Pages per request", tip, |ui| {
            let run = &mut ai.tasks.pages;
            let mut n = f32::from(run.per_request);
            let hi = f32::from(PageRun::PER_REQUEST_MAX);
            widgets::stepper(ui, &mut n, 1.0..=hi, 1.0, |v| format!("{v:.0}"), "pages");
            run.per_request = n.round().clamp(1.0, hi) as u8;
        });
    });
    note(
        ui,
        "Pages at once never passes what the model allows; see AI models.",
    );
    let chosen = ai.tasks.translate.language.as_deref();
    let name = chosen.and_then(mareader_settings::language_name);
    if let (Some(tag), Some(name)) = (chosen, name)
        && speaks(tag) == Some(false)
    {
        note(
            ui,
            &format!("Apple Intelligence doesn't write {name}. Pick a language it lists."),
        );
    }
}

/// What choosing a page task does: ask, this page, or on as one reads.
fn start_picker(ui: &mut Ui, salt: &str, start: &mut PageStart) {
    let name = |s: PageStart| match s {
        PageStart::Ask => "Ask each time",
        PageStart::Page => "This page",
        PageStart::Continuous => "Continuous",
    };
    widgets::combo(salt)
        .width(PICK_W)
        .selected_text(name(*start))
        .show_ui(ui, |ui| {
            for s in [PageStart::Ask, PageStart::Page, PageStart::Continuous] {
                ui.selectable_value(start, s, name(s));
            }
        });
}

/// A saved model for a task, or `none`.
fn preset_picker(
    ui: &mut Ui,
    salt: &str,
    presets: &[ModelPreset],
    chosen: &mut Option<PresetId>,
    none: &str,
) {
    let title = |id: PresetId| presets.iter().find(|p| p.id == id).map(ModelPreset::title);
    let shown = chosen.and_then(title).unwrap_or_else(|| none.to_owned());
    widgets::combo(salt)
        .width(PICK_W)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.selectable_value(chosen, None, none);
            for p in presets {
                ui.selectable_value(chosen, Some(p.id), p.title());
            }
        });
}

/// The book's language or a listed one; ones Apple can't write are greyed.
fn language_picker(
    ui: &mut Ui,
    (salt, none): (&str, &str),
    chosen: &mut Option<String>,
    speaks: &dyn Fn(&str) -> Option<bool>,
) {
    let name = chosen.as_deref().and_then(mareader_settings::language_name);
    widgets::combo(salt)
        .width(PICK_W)
        .height(320.0)
        .selected_text(name.unwrap_or(none))
        .show_ui(ui, |ui| {
            ui.selectable_value(chosen, None, none);
            for (tag, name) in LANGUAGES {
                let on = chosen.as_deref() == Some(tag);
                let button = egui::Button::selectable(on, name);
                let r = ui.add_enabled(speaks(tag) != Some(false), button);
                if r.clicked() {
                    *chosen = Some(tag.to_owned());
                }
                r.on_disabled_hover_text("Apple Intelligence doesn't write this language");
            }
        });
}
