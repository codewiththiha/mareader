//! The AI word card and the mark menu.

use egui::{
    Align, Align2, Context, Frame, Id, Layout, Margin, Rect, RichText, ScrollArea, Stroke, Ui, vec2,
};
use mareader_cefr::Role;
use mareader_engine::command::Command;
use mareader_engine::study::Action;
use mareader_library::{MarkId, Style};
use mareader_settings::{GlossDensity, HighlightSettings};
use mareader_study::{Failure, Pos, WordInfo};
use mareader_workspace::PaneId;

use crate::bubble::{Bubble, Side};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::z_order::{self, Tier};
use crate::{motion, widgets, word_card};

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
    /// The tagger's role: its class, until the AI names one, and its level.
    pub role: Option<&'a Role>,
    /// The class the word's dictionary rows name, once in.
    pub dictionary: Option<Pos>,
    pub answer: Answer<'a>,
    /// True when offline dictionary data exists for this word.
    pub show_tabs: bool,
    /// The word on screen.
    pub at: Rect,
    /// The pane it's kept inside.
    pub room: Rect,
    pub density: GlossDensity,
    /// Its word is being said aloud.
    pub saying: bool,
    /// How it sounds, once looked up.
    pub sound: &'a [mareader_engine::sounds::Line],
    /// A different meaning the AI dictionary kept for the word, to replace.
    pub kept: Option<&'a str>,
}

/// Draws the card under or over its word, pointing at it.
pub fn show(ctx: &Context, view: &View<'_>, t: &Tokens, out: &mut Commands) -> Rect {
    let id = word_card::bubble_id(view.word);
    let bubble = Bubble {
        id,
        at: view.at,
        room: view.room,
        width: word_card::WIDTH,
        margin: Margin::ZERO,
        snug: false,
        prefer: Side::Below,
    };
    bubble.show(ctx, t, |ui, cap| card(ui, view, cap, t, out)).0
}

fn card(ui: &mut Ui, view: &View<'_>, cap: f32, t: &Tokens, out: &mut Commands) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let top = ui.cursor().top();
    let said = match &view.answer {
        Answer::Ready(info, _) | Answer::Partial(info) => Some(info.pos.as_str()),
        _ => None,
    };
    let heard = mareader_engine::word_card::class(said, view.role, view.dictionary);
    let class = heard.map_or("", Pos::name);
    let head = word_card::Header {
        word: view.word,
        class,
        level: view.role.and_then(Role::cefr),
        sound: view.sound,
        say: Some(word_card::Say {
            saying: view.saying,
            voice: None,
        }),
        mode: word_card::Mode::Ai,
        show_tabs: view.show_tabs,
        close: Command::Study(Action::Close),
    };
    word_card::header(ui, &head, t, out);
    word_card::rule(ui, t);
    let comfy = view.density == GlossDensity::Comfortable;
    let pad_y = if comfy { 12 } else { word_card::PAD_Y };
    let used = ui.cursor().top() - top + f32::from(pad_y) * 2.0 + word_card::FOOT;
    let list = (cap - used).max(word_card::BODY_MIN);
    word_card::section(ui, pad_y, |ui| {
        ui.spacing_mut().item_spacing.y = if comfy { 8.0 } else { 5.0 };
        ScrollArea::vertical()
            .id_salt(("study-card", view.word))
            .max_height(list)
            .auto_shrink([false, true])
            .show(ui, |ui| body(ui, view, t, out));
    });
    word_card::rule(ui, t);
    footer(ui, view, t, out);
}

fn body(ui: &mut Ui, view: &View<'_>, t: &Tokens, out: &mut Commands) {
    match &view.answer {
        Answer::Waiting => shimmer(ui, t),
        Answer::Partial(info) => answer(ui, info, t),
        Answer::Ready(info, _) => {
            answer(ui, info, t);
            if let Some(kept) = view.kept {
                offer(ui, kept, t, out);
            }
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
            if e.why.retry() {
                ui.add_space(4.0);
                let retry = word_card::action_pill(ui, Some(Icon::Rescan), "Try again", true, t);
                if retry.clicked() {
                    out.push(Sent::Engine(Command::Study(Action::Retry)));
                }
            }
        }
    }
}

/// Pinned footer with attribution on the left and actions on the right.
fn footer(ui: &mut Ui, view: &View<'_>, t: &Tokens, out: &mut Commands) {
    word_card::footer_frame(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                word_card::pursue_button(ui, view.word, out);
                if let Answer::Ready(info, _) = &view.answer {
                    let copied = copy_text(view.word, info);
                    word_card::copy_button(ui, &copied, "Copy explanation", out);
                    let icon = Some(Icon::Rescan);
                    let again = word_card::footer_action(ui, icon, "Ask again", true, t);
                    widgets::tip(&again, "Ask the AI chosen in Settings", None);
                    if again.clicked() {
                        out.push(Sent::Engine(Command::Study(Action::AskAgain)));
                    }
                }
                ui.add_space(6.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    match &view.answer {
                        Answer::Ready(_, by) if !by.is_empty() => {
                            let label = RichText::new(*by).size(t.text_small).color(t.muted);
                            let resp = ui.add(egui::Label::new(label).truncate());
                            widgets::tip(&resp, by, None);
                        }
                        Answer::Waiting | Answer::Partial(_) => {
                            ui.label(
                                RichText::new("Asking AI…")
                                    .size(t.text_small)
                                    .color(t.muted),
                            );
                        }
                        _ => {}
                    }
                });
            });
        });
    });
}

/// The kept meaning and Replace; nothing is saved without the click.
fn offer(ui: &mut Ui, kept: &str, t: &Tokens, out: &mut Commands) {
    ui.add_space(4.0);
    heading(ui, "Your AI dictionary says", t);
    crate::shaped::label(ui, kept, t.text_small, t.muted);
    ui.add_space(4.0);
    let replace = word_card::action_pill(ui, None, "Replace with this answer", true, t);
    widgets::tip(&replace, "Save this answer in its place", None);
    if replace.clicked() {
        out.push(Sent::Engine(Command::Study(Action::ReplaceSaved)));
    }
}

fn copy_text(word: &str, info: &WordInfo) -> String {
    let mut out = format!("{word}: {}", info.meaning);
    if !info.synonyms.is_empty() {
        out.push_str("\nSynonyms: ");
        out.push_str(&info.synonyms.join(", "));
    }
    if !info.usages.is_empty() {
        out.push('\n');
        out.push_str(&info.usages.join("\n"));
    }
    out
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
    let width = ui.available_width().max(80.0);
    for share in [0.35, 1.0, 0.8] {
        let (rect, _) = ui.allocate_exact_size(vec2(width, 12.0), egui::Sense::hover());
        let bar = Rect::from_min_size(rect.min, vec2(width * share, rect.height()));
        ui.painter().rect_filled(bar, 4.0, tint);
    }
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
    let rect = z_order::area(ctx, Tier::Popover, id)
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
