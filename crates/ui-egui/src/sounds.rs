//! A card's sound lines drawn; the engine below reads and writes them.

use egui::text::{LayoutJob, TextFormat};
use egui::{FontId, Ui};
use mareader_engine::sounds::Line;

use crate::theme::{self, Tokens};

/// A card's sound lines, small and muted under its word.
pub fn show(ui: &mut Ui, lines: &[Line], t: &Tokens) {
    if lines.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        show_inline(ui, lines, t);
    });
}

/// Draws sound lines directly into an open horizontal row.
pub fn show_inline(ui: &mut Ui, lines: &[Line], t: &Tokens) {
    for line in lines {
        show_one(ui, line, t);
    }
}

/// Draws a single sound line without wrapping, preserving vertical alignment.
pub fn show_one(ui: &mut Ui, line: &Line, t: &Tokens) {
    let label = ui.add(egui::Label::new(layout(line, t)).selectable(false));
    if let Some(why) = &line.why {
        crate::widgets::tip(&label, why, None);
    }
}

/// Width a single sound line needs on one row.
pub fn width(ui: &Ui, line: &Line, t: &Tokens) -> f32 {
    ui.painter().layout_job(layout(line, t)).size().x
}

fn layout(line: &Line, t: &Tokens) -> LayoutJob {
    let size = t.text_small + 1.0;
    let plain = TextFormat::simple(FontId::proportional(size), t.ink);
    let strong = TextFormat::simple(theme::medium(size), t.ink);
    let quiet = TextFormat::simple(FontId::proportional(size), t.muted);
    // A stand-in accent's tag is amber, as its settings row warns.
    let tone = if line.why.is_some() {
        theme::caution(t)
    } else {
        t.muted
    };
    let tag = TextFormat::simple(theme::medium(t.text_small - 1.0), tone);
    let mut job = LayoutJob::default();
    if !line.tag.is_empty() {
        job.append(line.tag, 0.0, tag);
        job.append(" ", 0.0, quiet.clone());
    }
    let slash = if line.slashed { "/" } else { "" };
    job.append(slash, 0.0, quiet.clone());
    for (i, word) in line.words.iter().enumerate() {
        if i > 0 {
            job.append(" ", 0.0, plain.clone());
        }
        for (j, beat) in word.iter().enumerate() {
            if j > 0 {
                job.append("·", 0.0, quiet.clone());
            }
            let look = if beat.stressed { &strong } else { &plain };
            job.append(&beat.text, 0.0, look.clone());
        }
    }
    job.append(slash, 0.0, quiet);
    job
}
