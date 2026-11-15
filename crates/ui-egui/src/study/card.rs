//! The word card and the mark menu.

use egui::{
    Align, Align2, Context, Frame, Id, Layout, Margin, Order, Rect, RichText, ScrollArea, Stroke,
    Ui, vec2,
};
use mareader_cefr::Role;
use mareader_library::{MarkId, Style};
use mareader_settings::{GlossDensity, HighlightSettings};
use mareader_study::{Failure, WordInfo};
use mareader_workspace::PaneId;

use crate::bubble::{Bubble, Side};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::{motion, widgets};
use mareader_engine::command::Command;
use mareader_engine::study::Action;

const WIDTH: f32 = 320.0;

pub enum Answer<'a> {
    Waiting,
    /// Filling in as the model streams.
    Partial(&'a WordInfo),
    /// Done, with who answered; saved with the mark.
    Ready(&'a WordInfo, &'a str),
    Failed(&'a Failure),
}

pub struct View<'a> {
    pub word: &'a str,
    /// The tagger's role: its class until the AI's, and its level.
    pub role: Option<&'a Role>,
    pub answer: Answer<'a>,
    /// The word on screen.
    pub at: Rect,
    /// The pane it's kept inside.
    pub room: Rect,
    pub density: GlossDensity,
    /// Its word is being said aloud.
    pub saying: bool,
    /// How it sounds, once looked up.
    pub sound: &'a [crate::sounds::Line],
}

/// Draws the card under or over its word, pointing at it.
pub fn show(ctx: &Context, view: &View<'_>, t: &Tokens, out: &mut Commands) -> Rect {
    let comfy = view.density == GlossDensity::Comfortable;
    let bubble = Bubble {
        id: Id::new(("study-card", view.word)),
        at: view.at,
        room: view.room,
        width: WIDTH,
        margin: Margin::same(if comfy { 18 } else { 14 }),
        snug: false,
        prefer: Side::Below,
    };
    let shown = bubble.show(ctx, t, |ui, cap| {
        ui.spacing_mut().item_spacing.y = if comfy { 10.0 } else { 6.0 };
        // Only an answer taller than the room scrolls.
        ScrollArea::vertical()
            .max_height(cap)
            .auto_shrink([false, true])
            .show(ui, |ui| body(ui, view, t, out));
    });
    shown.0
}

fn body(ui: &mut Ui, view: &View<'_>, t: &Tokens, out: &mut Commands) {
    ui.horizontal(|ui| {
        if crate::shaped::needs_shaping(view.word) {
            crate::shaped::label(ui, view.word, t.text * 1.2, t.ink);
        } else {
            let word = RichText::new(view.word).size(t.text * 1.35).strong();
            ui.label(word.color(t.ink));
        }
        let said = match &view.answer {
            Answer::Ready(info, _) | Answer::Partial(info) => Some(info.pos.as_str()),
            _ => None,
        };
        let class = said
            .filter(|p| !p.is_empty())
            .or(view.role.map(Role::class));
        if let Some(class) = class {
            ui.label(RichText::new(class).italics().color(t.muted));
        }
        if let Some(level) = view.role.and_then(Role::cefr) {
            let tip = "This sense's CEFR level";
            let r = ui.label(
                RichText::new(level)
                    .size(t.text_small)
                    .strong()
                    .color(t.muted),
            );
            widgets::tip(&r, tip, None);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::small_icon_button(ui, Icon::Close, "Close").clicked() {
                out.push(Sent::Engine(Command::Study(Action::Close)));
            }
            crate::read_aloud::say_button(ui, view.word, (view.saying, None), out);
        });
    });
    crate::sounds::show(ui, view.sound, t);
    match &view.answer {
        Answer::Waiting => shimmer(ui, t),
        Answer::Partial(info) => {
            answer(ui, info, t);
            explaining(ui, t);
        }
        Answer::Ready(info, by) => {
            answer(ui, info, t);
            footer(ui, by, t, out);
        }
        Answer::Failed(e) => {
            ui.horizontal_wrapped(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                Icon::Alert.paint(ui, rect, t.danger);
                ui.label(RichText::new(e.why.message()).color(t.muted));
            });
            if let Some(detail) = &e.detail {
                ui.label(RichText::new(detail).size(t.text_small).color(t.muted));
            }
            if e.why.retry() && ui.button("Try again").clicked() {
                out.push(Sent::Engine(Command::Study(Action::Retry)));
            }
        }
    }
}

/// Who answered, and a way to ask the AI chosen now instead.
fn footer(ui: &mut Ui, by: &str, t: &Tokens, out: &mut Commands) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        if !by.is_empty() {
            ui.label(RichText::new(by).size(t.text_small).color(t.muted));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let text = RichText::new("Ask again")
                .size(t.text_small)
                .color(t.accent);
            let again = ui.add(egui::Button::new(text).frame(false));
            if again
                .on_hover_text("Ask the AI chosen in Settings")
                .clicked()
            {
                out.push(Sent::Engine(Command::Study(Action::AskAgain)));
            }
        });
    });
}

fn heading(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(text)
            .size(t.text_small)
            .strong()
            .color(t.muted),
    );
}

fn answer(ui: &mut Ui, info: &WordInfo, t: &Tokens) {
    if !info.meaning.is_empty() {
        heading(ui, "Meaning", t);
        crate::shaped::label(ui, &info.meaning, t.text, t.ink);
    }
    if !info.synonyms.is_empty() {
        heading(ui, "Synonyms", t);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 6.0);
            for s in &info.synonyms {
                chip(ui, s, t);
            }
        });
    }
    if !info.usages.is_empty() {
        heading(ui, "Usages", t);
        for u in &info.usages {
            // Hanging indent: wrapped lines start under the text.
            ui.horizontal_top(|ui| {
                ui.label(RichText::new("•").color(t.muted));
                if crate::shaped::needs_shaping(u) {
                    crate::shaped::label(ui, u, t.text, t.ink);
                } else {
                    ui.add(egui::Label::new(RichText::new(u).italics().color(t.ink)).wrap());
                }
            });
        }
    }
}

/// A synonym in a pill, moved whole to the next line when it doesn't fit.
fn chip(ui: &mut Ui, text: &str, t: &Tokens) {
    let pad = vec2(8.0, 3.0);
    // Only a word wider than the whole card wraps inside its pill.
    let widest = (ui.max_rect().width() - 2.0 * pad.x).max(0.0);
    let font = egui::FontId::proportional(t.text_small);
    let galley = ui.painter().layout(text.to_owned(), font, t.ink, widest);
    let size = galley.size() + 2.0 * pad;
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, t.radius, t.accent_soft);
    ui.painter().galley(rect.min + pad, galley, t.ink);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
}

/// Grey bars that breathe while the answer is on its way.
fn shimmer(ui: &mut Ui, t: &Tokens) {
    let breathe = motion::enabled(ui.ctx());
    let phase = if breathe {
        ui.ctx().request_repaint();
        (ui.input(|i| i.time) * 3.0).sin() as f32 * 0.5 + 0.5
    } else {
        0.5
    };
    let tint = t.line.gamma_multiply(0.6 + 0.4 * phase);
    for share in [0.35, 1.0, 0.8] {
        let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, 12.0), egui::Sense::hover());
        let bar = Rect::from_min_size(rect.min, vec2(WIDTH * share, rect.height()));
        ui.painter().rect_filled(bar, 4.0, tint);
    }
    explaining(ui, t);
}

fn explaining(ui: &mut Ui, t: &Tokens) {
    ui.label(
        RichText::new("Looking up…")
            .size(t.text_small)
            .color(t.muted),
    );
}

/// Below the words, else above, keeping a restyle in view.
fn menu_place(ctx: &Context, id: Id, at: egui::Pos2, words: Rect) -> (egui::Pos2, Align2) {
    let h = ctx
        .memory(|m| m.area_rect(id))
        .map_or(300.0, |r| r.height());
    let screen = ctx.content_rect();
    let below = words.bottom() + 6.0;
    let above = words.top() - 6.0;
    if !words.is_positive() {
        (at, Align2::LEFT_TOP)
    } else if below + h <= screen.bottom() - 4.0 {
        (egui::pos2(at.x, below), Align2::LEFT_TOP)
    } else if above - h >= screen.top() + 4.0 {
        (egui::pos2(at.x, above), Align2::LEFT_BOTTOM)
    } else {
        (at, Align2::LEFT_TOP)
    }
}

/// The right-click menu on a mark; returns where it went.
pub fn menu(
    ctx: &Context,
    (pane, mark): (PaneId, MarkId),
    (at, words): (egui::Pos2, Rect),
    noted: Option<bool>,
    (style, look): (Option<Style>, &HighlightSettings),
    t: &Tokens,
    out: &mut Commands,
) -> Rect {
    let id = Id::new("study-mark-menu");
    let (pos, pivot) = menu_place(ctx, id, at, words);
    let rect = egui::Area::new(id)
        .order(Order::Foreground)
        .pivot(pivot)
        .fixed_pos(pos)
        .constrain(true)
        .show(ctx, |ui| {
            Frame::menu(ui.style())
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius_lg)
                .inner_margin(Margin::same(6))
                .show(ui, |ui| {
                    // A highlight's look leads: pen, then colour.
                    let wide = style.is_some();
                    widgets::menu_style(ui, if wide { 224.0 } else { 200.0 });
                    if let Some(now) = style {
                        super::restyle::section(ui, mark, now, look, t, out);
                        widgets::menu_rule(ui);
                    }
                    if let Some(noted) = noted {
                        let label = if noted { "Edit note" } else { "Add note" };
                        let row = widgets::menu_row(ui, Icon::Note, label, None, false);
                        if row.clicked() {
                            out.push(Sent::Engine(Command::Study(Action::WriteNote(mark))));
                        }
                    }
                    let several =
                        widgets::menu_row(ui, Icon::SelectSeveral, "Select several", None, false);
                    if several.clicked() {
                        out.push(Sent::Engine(Command::Study(Action::SelectSeveral(
                            pane, mark,
                        ))));
                    }
                    widgets::menu_rule(ui);
                    if widgets::menu_row_danger(ui, Icon::Remove, "Remove highlight").clicked() {
                        out.push(Sent::Engine(Command::Study(Action::Remove(vec![mark]))));
                    }
                });
        })
        .response
        .rect;
    // A menu summoned here: toasts give it the room.
    crate::toast::keep_clear(ctx, rect);
    rect
}
