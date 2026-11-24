//! The deck's own buttons: rounded chips drawn to a measured size, so a row
//! of them sits exactly in the middle, and small blocks for the corners.

use std::sync::Arc;

use egui::{Align2, Color32, FontId, Galley, Pos2, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets;

/// How a chip stands out, most to least.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Primary,
    Outline,
    Soft,
    Quiet,
}

/// One button of a row: its words, mark, key and look.
pub struct Chip<'a> {
    pub label: &'a str,
    /// Drawn instead of the words, which stay its name.
    pub icon_only: bool,
    pub icon: Option<Icon>,
    /// A key cap after the words, as the shortcut.
    pub key: Option<&'a str>,
    pub look: Look,
    pub height: f32,
    pub width: Option<f32>,
    pub enabled: bool,
}

impl<'a> Chip<'a> {
    pub fn new(label: &'a str, look: Look) -> Self {
        Self {
            label,
            icon_only: false,
            icon: None,
            key: None,
            look,
            height: 34.0,
            width: None,
            enabled: true,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Only the mark shows; the label names it for screen readers.
    pub fn bare(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self.icon_only = true;
        self
    }

    pub fn key(mut self, key: &'a str) -> Self {
        self.key = Some(key);
        self
    }

    pub fn tall(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn wide(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn font(&self) -> FontId {
        let size = if self.height >= 38.0 { 15.0 } else { 14.0 };
        match self.look {
            Look::Primary | Look::Outline => theme::medium(size),
            Look::Soft | Look::Quiet => FontId::proportional(size),
        }
    }

    fn galley(&self, ui: &Ui) -> Option<Arc<Galley>> {
        let text = (!self.icon_only).then(|| self.label.to_owned())?;
        let painter = ui.painter();
        Some(painter.layout_no_wrap(text, self.font(), Color32::PLACEHOLDER))
    }

    fn key_galley(&self, ui: &Ui) -> Option<Arc<Galley>> {
        let key = self.key.filter(|_| !self.icon_only)?.to_owned();
        let font = theme::medium(11.0);
        Some(ui.painter().layout_no_wrap(key, font, Color32::PLACEHOLDER))
    }

    /// The words and key cap laid out, and the width all parts take.
    fn parts(&self, ui: &Ui) -> (Option<Arc<Galley>>, Option<Arc<Galley>>, f32) {
        let words = self.galley(ui);
        let cap = self.key_galley(ui);
        let widths = [
            self.icon.map(|_| ICON),
            words.as_ref().map(|g| g.size().x),
            cap.as_ref().map(|g| cap_width(g)),
        ];
        let shown: Vec<f32> = widths.into_iter().flatten().collect();
        let gaps = PART_GAP * shown.len().saturating_sub(1) as f32;
        (words, cap, shown.iter().sum::<f32>() + gaps)
    }

    /// The width it takes, padding and all.
    pub fn measure(&self, ui: &Ui) -> f32 {
        if let Some(width) = self.width {
            return width;
        }
        if self.icon_only {
            return self.height;
        }
        self.parts(ui).2 + 2.0 * (self.height * 0.5).max(14.0)
    }

    /// Draws it at `rect`, answering presses while enabled.
    pub fn show(self, ui: &mut Ui, rect: Rect) -> Response {
        let t = Tokens::get(ui.ctx());
        let sense = match self.enabled {
            true => Sense::click(),
            false => Sense::hover(),
        };
        let resp = ui.interact(rect, ui.id().with(("chip", self.label)), sense);
        let hot = self.enabled && resp.hovered();
        let (fill, edge, ink) = colors(self.look, hot, &t);
        let fade = if self.enabled { 1.0 } else { 0.4 };
        let (fill, ink) = (fill.gamma_multiply(fade), ink.gamma_multiply(fade));
        let painter = ui.painter();
        let radius = rect.height() / 2.0;
        painter.rect_filled(rect, radius, fill);
        if let Some(edge) = edge {
            let stroke = Stroke::new(1.0, edge.gamma_multiply(fade));
            painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
        }
        self.paint_parts(ui, rect, ink, &t);
        let (label, enabled) = (self.label, self.enabled);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
        if self.icon_only {
            widgets::tip(&resp, label, self.key);
        }
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }

    /// The mark, words and key cap, side by side in the middle of `rect`.
    fn paint_parts(&self, ui: &Ui, rect: Rect, ink: Color32, t: &Tokens) {
        let (words, cap, inner) = self.parts(ui);
        let mut x = rect.center().x - inner / 2.0;
        let y = rect.center().y;
        if let Some(icon) = self.icon {
            let at = Rect::from_center_size(pos2(x + ICON / 2.0, y), vec2(ICON, ICON));
            icon.paint(ui, at, ink);
            x += ICON + PART_GAP;
        }
        if let Some(words) = words {
            let top = pos2(x, y - words.size().y / 2.0);
            x += words.size().x + PART_GAP;
            ui.painter().galley(top, words, ink);
        }
        if let Some(cap) = cap {
            key_cap(ui, pos2(x, y), cap, t);
        }
    }
}

/// Room between a chip's mark, words and key cap.
const PART_GAP: f32 = 7.0;
/// A mark's side inside a chip.
const ICON: f32 = 16.0;

fn cap_width(galley: &Galley) -> f32 {
    (galley.size().x + 10.0).max(20.0)
}

/// A key cap with its left middle at `at`.
fn key_cap(ui: &Ui, at: Pos2, galley: Arc<Galley>, t: &Tokens) {
    let size = vec2(cap_width(&galley), 20.0);
    let rect = Rect::from_min_size(at - vec2(0.0, size.y / 2.0), size);
    let painter = ui.painter();
    painter.rect_filled(rect, 5.0, t.surface);
    let edge = Stroke::new(1.0, t.line);
    painter.rect_stroke(rect, 5.0, edge, egui::StrokeKind::Inside);
    let top = rect.center() - galley.size() / 2.0;
    painter.galley(top, galley, t.muted);
}

/// Fill, edge and ink for `look`, `hot` while hovered.
fn colors(look: Look, hot: bool, t: &Tokens) -> (Color32, Option<Color32>, Color32) {
    match look {
        Look::Primary => {
            let fill = match hot {
                true => t.accent.gamma_multiply(0.9),
                false => t.accent,
            };
            (fill, None, theme::ink_on(t.accent))
        }
        Look::Outline => (if hot { t.hover } else { t.surface }, Some(t.line), t.ink),
        Look::Soft => (if hot { t.pressed } else { t.hover }, None, t.ink),
        Look::Quiet => {
            let fill = if hot { t.hover } else { Color32::TRANSPARENT };
            (fill, None, if hot { t.ink } else { t.muted })
        }
    }
}

/// Lays `chips` out across `ui`'s width, the row centred, `gap` apart.
pub fn row(ui: &mut Ui, chips: Vec<Chip<'_>>, gap: f32) -> Vec<Response> {
    spaced(ui, chips, &[gap])
}

/// A centred row whose gaps go in order, the last one repeating.
pub fn spaced(ui: &mut Ui, chips: Vec<Chip<'_>>, gaps: &[f32]) -> Vec<Response> {
    let widths: Vec<f32> = chips.iter().map(|c| c.measure(ui)).collect();
    let height = chips.iter().map(|c| c.height).fold(0.0, f32::max);
    let last = gaps.last().copied().unwrap_or(0.0);
    let gap = |i: usize| gaps.get(i).copied().unwrap_or(last);
    let spaced: f32 = (0..widths.len().saturating_sub(1)).map(gap).sum();
    let total = widths.iter().sum::<f32>() + spaced;
    let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let mut x = band.center().x - total / 2.0;
    let mut out = Vec::new();
    for (i, (chip, width)) in chips.into_iter().zip(widths).enumerate() {
        let top = band.center().y - chip.height / 2.0;
        let rect = Rect::from_min_size(pos2(x, top), vec2(width, chip.height));
        out.push(chip.show(ui, rect));
        x += width + gap(i);
    }
    out
}

/// A round block holding `icon`, named and tipped `label`.
pub fn round(ui: &mut Ui, rect: Rect, icon: Icon, label: &str, t: &Tokens) -> Response {
    let resp = ui.interact(rect, ui.id().with(("deck-round", label)), Sense::click());
    plate(ui, rect, t);
    if resp.hovered() {
        let r = rect.height() / 2.0 - 1.0;
        ui.painter().circle_filled(rect.center(), r, t.hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let ink = if resp.hovered() { t.ink } else { t.muted };
    let mark = Rect::from_center_size(rect.center(), vec2(ICON, ICON));
    icon.paint(ui, mark, ink);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    widgets::tip(&resp, label, None);
    resp
}

/// A small block of words and a mark, `align` saying which corner is `at`.
pub fn block(ui: &Ui, at: Pos2, align: Align2, icon: Option<Icon>, text: &str, t: &Tokens) -> Rect {
    let font = theme::medium(14.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, t.ink);
    let size = vec2(block_width(ui, icon.is_some(), text), BLOCK);
    let rect = align.anchor_size(at, size);
    plate(ui, rect, t);
    let mut x = rect.left() + 14.0;
    if let Some(icon) = icon {
        let side = ICON - 2.0;
        let r = Rect::from_center_size(pos2(x + side / 2.0, rect.center().y), vec2(side, side));
        icon.paint(ui, r, t.muted);
        x += side + PART_GAP;
    }
    let top = pos2(x, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(top, galley, t.ink);
    rect
}

/// The width a corner block of `text` takes, with a mark when `icon`.
pub fn block_width(ui: &Ui, icon: bool, text: &str) -> f32 {
    let font = theme::medium(14.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let mark = if icon { ICON - 2.0 + PART_GAP } else { 0.0 };
    galley.size().x + mark + 28.0
}

/// The height of a corner block.
pub const BLOCK: f32 = 34.0;

/// A corner block's raised paper.
pub fn plate(ui: &Ui, rect: Rect, t: &Tokens) {
    let painter = ui.painter();
    let shade = egui::Shadow {
        offset: [0, 3],
        blur: 12,
        spread: 0,
        color: t.shadow.gamma_multiply(0.7),
    };
    painter.add(shade.as_shape(rect, rect.height() / 2.0));
    painter.rect_filled(rect, rect.height() / 2.0, t.surface);
    let edge = Stroke::new(1.0, t.line);
    painter.rect_stroke(rect, rect.height() / 2.0, edge, egui::StrokeKind::Inside);
}
