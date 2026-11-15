//! The one modal: a centred card over a dimmed window, and its parts.

use egui::{
    Align, Button, Color32, Context, Frame, Id, Label, Layout, Margin, Modal, ModalResponse, Rect,
    RichText, Stroke, Ui, pos2, vec2,
};

use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets;

/// Room for the switch beside a switch card's words.
const SWITCH_ROOM: f32 = 48.0;

/// A modal card, always in the middle of the window.
pub struct Dialog {
    id: Id,
    margin: i8,
}

impl Dialog {
    pub fn new(id: impl egui::AsId) -> Self {
        Self {
            id: Id::new(id),
            margin: 16,
        }
    }

    /// The card's inner margin; panels that draw to their edges pass 0.
    pub fn margin(mut self, margin: i8) -> Self {
        self.margin = margin;
        self
    }

    /// Draws the card; a click outside or Escape asks it to close.
    pub fn show<R>(self, ctx: &Context, add: impl FnOnce(&mut Ui) -> R) -> ModalResponse<R> {
        let t = Tokens::get(ctx);
        let frame = Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(1.0, t.line))
            .corner_radius(t.radius_lg)
            .inner_margin(Margin::same(self.margin))
            .shadow(ctx.global_style().visuals.window_shadow);
        // egui's own area: centred, keyed by the dialog, so focus stays in.
        Modal::new(self.id)
            .frame(frame)
            .backdrop_color(backdrop(&t))
            .show(ctx, add)
    }
}

/// Dim enough to set the card apart, light enough to keep the place.
fn backdrop(t: &Tokens) -> Color32 {
    Color32::from_black_alpha(if t.dark { 90 } else { 48 })
}

/// A card's heading alone.
pub fn title(ui: &mut Ui, text: &str, t: &Tokens) {
    heading(ui, None, text, None, t);
}

/// An optional mark, the question, and a quieter line under it.
pub fn heading(
    ui: &mut Ui,
    mark: Option<(Icon, Color32)>,
    title: &str,
    message: Option<&str>,
    t: &Tokens,
) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if let Some((icon, tint)) = mark {
            ui.add(icon.image(ui, 18.0).tint(tint));
        }
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let text = RichText::new(title).font(theme::medium(15.0)).color(t.ink);
            ui.add(Label::new(text).wrap());
            if let Some(message) = message {
                ui.add(Label::new(RichText::new(message).color(t.muted)).wrap());
            }
        });
    });
}

/// A right-aligned row: the first button added sits rightmost.
pub fn buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(Layout::right_to_left(Align::Center), add);
}

/// The filled button that does the thing.
pub fn primary(ui: &mut Ui, label: &str, fill: Color32, t: &Tokens) -> egui::Response {
    let button = widgets::primary_button(ui, None, label, fill, t).min_size(vec2(84.0, 30.0));
    ui.add(button)
}

/// A quiet button beside it.
pub fn quiet(ui: &mut Ui, label: &str, t: &Tokens) -> egui::Response {
    let button = Button::new(RichText::new(label).color(t.ink))
        .corner_radius(t.radius)
        .min_size(vec2(72.0, 30.0));
    ui.add(button)
}

/// A switch with its meaning; `warn` tints the card while it's on.
pub fn switch_card(
    ui: &mut Ui,
    on: &mut bool,
    label: &str,
    note: &str,
    warn: Option<Color32>,
    t: &Tokens,
) {
    let hot = warn.filter(|_| *on);
    let fill = hot.map_or(Color32::TRANSPARENT, |c| c.gamma_multiply(0.1));
    let line = hot.map_or(t.line, |c| c.gamma_multiply(0.5));
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, line))
        .corner_radius(t.radius)
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            // Words in the flow, the switch beside them: a row would centre
            // the words low.
            ui.set_width(ui.available_width());
            let room = (ui.available_width() - SWITCH_ROOM).max(0.0);
            let layout = Layout::top_down(Align::Min);
            let words = ui.allocate_ui_with_layout(vec2(room, 0.0), layout, |ui| {
                ui.set_width(room);
                ui.spacing_mut().item_spacing.y = 2.0;
                let ink = hot.unwrap_or(t.ink);
                ui.add(Label::new(RichText::new(label).color(ink)).wrap());
                let small = RichText::new(note).size(t.text_small).color(t.muted);
                ui.add(Label::new(small).wrap());
            });
            let mid = pos2(ui.max_rect().right() - 18.0, words.response.rect.center().y);
            let at = Rect::from_center_size(mid, vec2(36.0, 20.0));
            ui.put(at, |ui: &mut Ui| widgets::switch(ui, on, label));
        });
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// Rust files under `dir`, with their text.
    fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                out.push((path.display().to_string(), text));
            }
        }
    }

    #[test]
    fn every_modal_goes_through_the_dialog() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        sources(&src, &mut files);
        assert!(files.len() > 10, "found the sources");
        let own = |p: &str| p.ends_with("modal.rs");
        let needle = ["Modal", "::new("].concat();
        let stray: Vec<&str> = (files.iter())
            .filter(|(p, text)| !own(p) && text.contains(&needle))
            .map(|(p, _)| p.as_str())
            .collect();
        assert!(stray.is_empty(), "use modal::Dialog in {stray:?}");
    }
}
