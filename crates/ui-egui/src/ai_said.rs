//! What an AI wrote of a word: the meaning, synonyms and usages.
//!
//! The AI word card and an AI dictionary row on a lookup card share it.

use egui::{RichText, Ui, vec2};

use crate::theme::Tokens;

/// A meaning, its synonyms and its usages.
pub type Said<'a> = (&'a str, &'a [String], &'a [String]);

/// A small strong title over a part.
pub fn heading(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(text)
            .size(t.text_small)
            .strong()
            .color(t.muted),
    );
}

/// Each part that has something, under its title.
pub fn show(ui: &mut Ui, (meaning, synonyms, usages): Said<'_>, t: &Tokens) {
    if !meaning.is_empty() {
        heading(ui, "Meaning", t);
        crate::shaped::label(ui, meaning, t.text, t.ink);
    }
    if !synonyms.is_empty() {
        heading(ui, "Synonyms", t);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 6.0);
            for s in synonyms {
                chip(ui, s, t);
            }
        });
    }
    if !usages.is_empty() {
        heading(ui, "Usages", t);
        for u in usages {
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
