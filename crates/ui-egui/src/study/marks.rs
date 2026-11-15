//! Marks on the pages on screen: where each lands, drawn and hit.

use std::ops::Range;
use std::sync::Arc;

use egui::{Color32, Id, Painter, Pos2, Rect, Stroke, StrokeKind, Ui, pos2};
use mareader_appearance::{MarkFill, PageTone, Rgb};
use mareader_engine::study::words;
use mareader_library::{MarkId, Pen, Spot};
use mareader_pdf::{Bounds, TextLayer};
use mareader_session::{Caret, Selection};

use crate::gpu::{PagePass, PagesCallback, Quad};

/// How a mark is drawn, resolved from its record and the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ink {
    pub pen: Pen,
    pub color: Rgb,
    /// An Explain mark: drawn last, and a click opens its card.
    pub ai: bool,
}

/// A mark to draw: which, where and how.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub id: MarkId,
    pub spot: Spot,
    pub ink: Ink,
    /// It has a note: a badge marks where it ends.
    pub noted: bool,
}

/// One mark on screen: a box per line it covers.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: MarkId,
    pub rects: Vec<Rect>,
    pub ink: Ink,
    /// A noted mark's badge, after its last line.
    pub badge: Option<Rect>,
}

impl Placed {
    pub fn contains(&self, p: Pos2) -> bool {
        let on_badge = self.badge.is_some_and(|b| b.expand(2.0).contains(p));
        on_badge || self.rects.iter().any(|r| r.expand(2.0).contains(p))
    }

    /// Every box at once, for a card to sit beside.
    pub fn bounds(&self) -> Rect {
        let all = self.rects.iter().copied().reduce(|a, b| a.union(b));
        all.unwrap_or(Rect::NOTHING)
    }
}

/// Where `marks` fall on the pages on screen; ones off screen are left out.
pub fn place(marks: &[Shown], pages: &crate::selection::Pages<'_>) -> Vec<Placed> {
    let mut out = Vec::new();
    for sheet in pages.sheets {
        let layer = pages.texts.get(pages.doc, sheet.page);
        // A book's page holds a run of blocks; marks outside it are skipped.
        let span = layer.as_ref().and_then(|l| {
            let first = (0..l.places.len()).find_map(|i| l.place(i))?;
            let last = (0..l.places.len()).rev().find_map(|i| l.place(i))?;
            Some((first, last))
        });
        // The text's right edge on screen, worked out once a note needs it.
        let mut edge: Option<Option<f32>> = None;
        for mark in marks {
            // A page mark's own box, or the run of characters it covers.
            let (own, chars) = match mark.spot {
                Spot::Page { page, rect } if page == sheet.page => {
                    let [x, y, w, h] = rect;
                    let b = Bounds {
                        x0: x,
                        y0: y,
                        x1: x + w,
                        y1: y + h,
                    };
                    (Some(b), 0..0)
                }
                Spot::Text { block, start, end } => {
                    let (Some(layer), Some((first, last))) = (&layer, span) else {
                        continue;
                    };
                    if (block, end) <= first || (block, start) > last {
                        continue;
                    }
                    let Some(chars) = words::find(layer, block, start..end) else {
                        continue;
                    };
                    (None, chars)
                }
                Spot::Chars { from, to } => {
                    let Some(layer) = &layer else { continue };
                    let Some(chars) = chars(from, to).on_page(sheet.page, layer.text.len()) else {
                        continue;
                    };
                    (None, chars)
                }
                Spot::Blocks { from, to } => {
                    let (Some(layer), Some((first, last))) = (&layer, span) else {
                        continue;
                    };
                    let (from, to) = ((from[0], from[1]), (to[0], to[1]));
                    if to <= first || from > last {
                        continue;
                    }
                    let Some(chars) = between(layer, from, to) else {
                        continue;
                    };
                    (None, chars)
                }
                Spot::Page { .. } => continue,
            };
            let lines = layer.iter().flat_map(|l| l.text.line_boxes(chars.clone()));
            let rects: Vec<Rect> = (own.into_iter().chain(lines))
                .map(|b| sheet.to_screen(b).intersect(sheet.shown))
                .filter(|r| r.is_positive())
                .collect();
            if !rects.is_empty() {
                let column = || {
                    let lines = layer.as_ref()?.text.line_bounds();
                    lines.map(|b| sheet.to_screen(b).right()).reduce(f32::max)
                };
                let badge = match rects.last() {
                    Some(r) if mark.noted => {
                        let edge = *edge.get_or_insert_with(column);
                        Some(badge(*r, edge, sheet.rect))
                    }
                    _ => None,
                };
                out.push(Placed {
                    id: mark.id,
                    rects,
                    ink: mark.ink,
                    badge,
                });
            }
        }
    }
    out
}

/// A badge in the margin beside the line, else raised over its end.
fn badge(line: Rect, edge: Option<f32>, page: Rect) -> Rect {
    let d = (line.height() * 0.8).clamp(10.0, 18.0);
    let x = edge.unwrap_or(line.right()).max(line.right()) + 6.0 + d * 0.5;
    if x + d * 0.5 <= page.right() - 2.0 {
        let centre = pos2(x, line.center().y);
        return Rect::from_center_size(centre, egui::Vec2::splat(d));
    }
    let d = (line.height() * 0.5).clamp(8.0, 12.0);
    let centre = pos2(line.right(), line.top());
    Rect::from_center_size(centre, egui::Vec2::splat(d))
}

/// A PDF highlight's span, as the selection it was made from.
pub fn chars(from: [usize; 2], to: [usize; 2]) -> Selection {
    let caret = |[page, index]: [usize; 2]| Caret { page, index };
    Selection {
        anchor: caret(from),
        focus: caret(to),
    }
}

/// The chars whose `(block, byte)` lie from `from` up to `to`.
fn between(layer: &TextLayer, from: (usize, usize), to: (usize, usize)) -> Option<Range<usize>> {
    let within = |i: usize| layer.place(i).is_some_and(|p| from <= p && p < to);
    let n = layer.places.len();
    let start = (0..n).find(|&i| within(i))?;
    let end = (start..n).rev().find(|&i| within(i))?;
    Some(start..end + 1)
}

/// Each mark's boxes in its pen, at `opacity`.
pub fn add(placed: &[Placed], opacity: f32, under: &mut Under) {
    for mark in placed {
        for r in &mark.rects {
            under.add(*r, mark.ink.pen, mark.ink.color, opacity);
        }
    }
}

/// Noted marks' badges, over the page and its fills.
pub fn badges(ui: &Ui, placed: &[Placed]) {
    let page = crate::theme::Tokens::get(ui.ctx()).page;
    let (paper, ink_tone) = page.paper_and_ink();
    let brightness = |c: [f32; 3]| c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114;
    let dark_page = brightness(paper) < brightness(ink_tone);
    let mut seen: Vec<MarkId> = Vec::new();
    for mark in placed {
        let again = seen.iter().filter(|id| **id == mark.id).count();
        seen.push(mark.id);
        if let (Some(b), Some(last)) = (mark.badge, mark.rects.last()) {
            let fill = crate::theme::color(mark.ink.color);
            let tab_ink = if dark_page {
                fill.lerp_to_gamma(Color32::WHITE, 0.2)
            } else {
                fill.lerp_to_gamma(Color32::BLACK, 0.38)
            };
            for r in &mark.rects {
                let bar = Rect::from_min_max(pos2(r.left(), r.bottom() - 2.0), r.max);
                ui.painter().rect_filled(bar, 1.0, tab_ink);
            }
            if mark.ink.pen == Pen::Underline {
                let right = (last.right() + 5.0).min(b.right());
                let tip = Rect::from_min_max(
                    pos2(last.right(), last.bottom() - 2.5),
                    pos2(right, last.bottom() + 0.5),
                );
                ui.painter().rect_filled(tip, 1.0, tab_ink);
            }
            let stop = ui.id().with(("stop-mark", mark.id, again));
            let kb = crate::focus::keyboard(ui.ctx());
            let focused = kb && ui.ctx().memory(|m| m.has_focus(stop));
            let hovering = ui
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|p| mark.contains(p));
            if hovering || focused {
                note_badge(ui, b, mark.ink.color);
            }
        }
    }
}

/// A note's badge: a disc in the mark's colour holding a note glyph.
fn note_badge(ui: &Ui, b: Rect, color: Rgb) {
    let fill = crate::theme::color(color);
    let edge = fill.lerp_to_gamma(Color32::BLACK, 0.35);
    let r = b.width() * 0.5;
    ui.painter()
        .circle(b.center(), r, fill, Stroke::new(1.0, edge));
    // Dark ink on a light disc, light on a dark one.
    let Rgb(red, green, blue) = color;
    let light = 0.299 * f32::from(red) + 0.587 * f32::from(green) + 0.114 * f32::from(blue);
    let ink = if light > 140.0 {
        Color32::from_black_alpha(200)
    } else {
        Color32::from_white_alpha(230)
    };
    let glyph = Rect::from_center_size(b.center(), egui::Vec2::splat(b.width() * 0.62));
    crate::icons::Icon::Note.paint(ui, glyph, ink);
}

/// One page's marks: fills laid under its ink, underlines over it.
pub struct Under {
    tone: PageTone,
    fills: Vec<Quad>,
    /// Boxes on dim pages, redrawn with their text lifted: box, tone, tint.
    lifts: Vec<(Rect, PageTone, Color32)>,
    lines: Vec<(Rect, Color32)>,
    dots: Vec<(Pos2, f32, Color32)>,
    /// Each colour and strength's fill, worked out once.
    seen: Vec<((Rgb, u32), MarkFill)>,
}

impl Under {
    /// Marks for pages drawn in `tone`.
    pub fn new(tone: PageTone) -> Self {
        Self {
            tone,
            fills: Vec::new(),
            lifts: Vec::new(),
            lines: Vec::new(),
            dots: Vec::new(),
            seen: Vec::new(),
        }
    }

    /// One line's box in `pen`: a square highlight, or a solid underline.
    pub fn add(&mut self, r: Rect, pen: Pen, color: Rgb, opacity: f32) {
        let area = pen_area(r, pen);
        if pen == Pen::Underline {
            self.lines.push((area, crate::theme::color(color)));
            return;
        }
        let key = (color, opacity.to_bits());
        let fill = match self.seen.iter().find(|(k, _)| *k == key) {
            Some((_, fill)) => *fill,
            None => {
                let fill = self.tone.mark(color.to_unit(), opacity);
                self.seen.push((key, fill));
                fill
            }
        };
        match fill.tone() {
            // Without the page pass, a lifted box falls back to a light wash.
            Some(tone) => self
                .lifts
                .push((area, tone, tint(color.to_unit(), opacity))),
            None => self
                .fills
                .push(Quad::fill(area, tint(fill.color, fill.strength))),
        }
    }

    /// A graded vocabulary mark, with a non-colour cue to its level.
    pub fn level_mark(&mut self, r: Rect, band: u8, style: mareader_settings::CefrMark, lift: f32) {
        use mareader_settings::CefrMark;
        let color = match band {
            1 => Rgb(0x5b, 0x8f, 0x3a),
            2 => Rgb(0x7c, 0x9a, 0x2e),
            3 => Rgb(0xc2, 0x8a, 0x12),
            4 => Rgb(0xd0, 0x6a, 0x1c),
            5 => Rgb(0xc2, 0x41, 0x2d),
            _ => Rgb(0x9b, 0x2c, 0x5a),
        };
        match style {
            CefrMark::Wash => {
                let strength = 0.12 + f32::from(band.min(6)) * 0.025 + lift;
                self.add(r, Pen::Full, color, strength);
            }
            CefrMark::Underline => {
                let thickness = (r.height() * if band >= 5 { 0.14 } else { 0.08 }).max(1.0);
                let ink = self.level_ink(color);
                let line = Rect::from_min_max(pos2(r.left(), r.bottom() - thickness), r.max);
                if band == 3 {
                    let mut x = line.left();
                    while x < line.right() {
                        let right = (x + thickness).min(line.right());
                        self.lines.push((
                            Rect::from_min_max(pos2(x, line.top()), pos2(right, line.bottom())),
                            ink,
                        ));
                        x += thickness * 2.5;
                    }
                } else {
                    self.lines.push((line, ink));
                }
            }
            CefrMark::Dot => {
                let radius = (r.height() * (0.08 + f32::from(band.min(6)) * 0.012)).clamp(1.5, 3.5);
                self.dots.push((
                    pos2(r.center().x, r.top() - radius - 1.0),
                    radius,
                    self.level_ink(color),
                ));
            }
            CefrMark::Custom => {}
        }
    }

    fn level_ink(&self, color: Rgb) -> Color32 {
        let (paper, ink) = self.tone.paper_and_ink();
        let brightness = |c: [f32; 3]| c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114;
        let color = crate::theme::color(color);
        if brightness(paper) < brightness(ink) {
            color.lerp_to_gamma(Color32::WHITE, 0.38)
        } else {
            color
        }
    }

    /// Draws them within `clip` over `pages`, the quads just drawn.
    pub fn show(self, ui: &Ui, pass: Option<&Arc<PagePass>>, clip: Rect, pages: &[Quad]) {
        let blend = self.tone.mark([1.0; 3], 1.0).blend;
        match pass {
            Some(pass) => {
                if !self.fills.is_empty() {
                    let shape = PagesCallback::marks(pass.clone(), clip, self.fills, blend);
                    ui.painter().add(shape);
                }
                let lifted = lifted(&self.lifts, pages);
                if !lifted.is_empty() {
                    let shape = PagesCallback::shape(pass.clone(), clip, lifted);
                    ui.painter().add(shape);
                }
            }
            None => {
                let lifts = self.lifts.iter().map(|(r, _, tint)| (*r, *tint));
                let fills = self.fills.iter().map(|q| (q.rect, q.tint));
                for (rect, tint) in fills.chain(lifts) {
                    ui.painter().rect_filled(rect, 0.0, tint);
                }
            }
        }
        for (line, colour) in self.lines {
            ui.painter().rect_filled(line, 0.0, colour);
        }
        for (centre, radius, colour) in self.dots {
            ui.painter().circle_filled(centre, radius, colour);
        }
    }
}

/// A premultiplied fill of `color` at `strength`.
fn tint(color: [f32; 3], strength: f32) -> Color32 {
    let [r, g, b] = color.map(|c| (c * 255.0).round() as u8);
    let a = (strength * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Each lifted box's page quads, clipped to it and in its tone.
fn lifted(lifts: &[(Rect, PageTone, Color32)], pages: &[Quad]) -> Vec<Quad> {
    let mut out = Vec::new();
    for (area, tone, _) in lifts {
        let start = out.len();
        for q in pages.iter().filter(|q| q.texture.is_some()) {
            let clip = q.clip.intersect(*area);
            if clip.is_positive() {
                out.push(Quad {
                    clip,
                    tone: *tone,
                    ..q.clone()
                });
            }
        }
        if out.len() == start {
            out.push(Quad::fill(
                *area,
                crate::theme::color(Rgb::from_unit(tone.paper)),
            ));
        }
    }
    out
}

/// Where `pen` inks a line's box `r`: all, its lower half or a rule.
pub fn pen_area(r: Rect, pen: Pen) -> Rect {
    match pen {
        Pen::Full => r,
        Pen::Half => Rect::from_min_max(pos2(r.left(), r.center().y), r.max),
        Pen::Underline => {
            let thick = (r.height() * 0.08).max(1.0);
            Rect::from_min_max(pos2(r.left(), r.bottom() - thick), r.max)
        }
    }
}

/// The mark under `p`, the last drawn first.
pub fn hit(placed: &[Placed], p: Pos2) -> Option<&Placed> {
    placed.iter().rev().find(|m| m.contains(p))
}

/// A still press held this long on a mark picks it.
pub const LONG_PRESS: f64 = 0.5;
/// Set when a long press picked: its release's click does nothing.
const ARMED: &str = "mark-long-press";

/// The mark a still press has held long enough on, once per press.
pub fn long_press(ui: &Ui, placed: &[Placed]) -> Option<MarkId> {
    let id = Id::new(ARMED);
    let (origin, start, down, latest, now) = ui.input(|i| {
        let p = &i.pointer;
        let down = p.primary_down();
        (
            p.press_origin(),
            p.press_start_time(),
            down,
            p.latest_pos(),
            i.time,
        )
    });
    let (Some(origin), Some(start), true) = (origin, start, down) else {
        return None;
    };
    let mark = hit(placed, origin)?;
    let still = latest.is_some_and(|p| p.distance(origin) < 6.0);
    if !still || ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false) {
        return None;
    }
    let held = now - start;
    if held < LONG_PRESS {
        ui.ctx()
            .request_repaint_after_secs((LONG_PRESS - held) as f32);
        return None;
    }
    ui.data_mut(|d| d.insert_temp(id, true));
    Some(mark.id)
}

/// True when this frame's click ends a long press.
pub fn swallowed(ui: &Ui, clicked: bool) -> bool {
    let id = Id::new(ARMED);
    if ui.input(|i| i.pointer.any_pressed()) {
        ui.data_mut(|d| d.remove::<bool>(id));
    }
    let armed = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if clicked && armed {
        ui.data_mut(|d| d.remove::<bool>(id));
    }
    clicked && armed
}

/// A ring around each picked mark.
pub fn ring(painter: &Painter, placed: &[Placed], picked: &[MarkId], color: Color32) {
    let stroke = Stroke::new(1.5, color);
    for m in placed.iter().filter(|m| picked.contains(&m.id)) {
        for r in &m.rects {
            painter.rect_stroke(r.expand(2.0), 2.0, stroke, StrokeKind::Outside);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_badge_sits_in_the_margin_or_raised_over_the_end() {
        let line = Rect::from_min_max(pos2(100.0, 50.0), pos2(200.0, 70.0));
        let page = Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 600.0));
        let b = badge(line, Some(300.0), page);
        assert!(b.left() > 300.0, "clear of the column: {b:?}");
        assert!((b.center().y - line.center().y).abs() < 0.5);
        let tight = Rect::from_min_max(pos2(0.0, 0.0), pos2(305.0, 600.0));
        let b = badge(line, Some(300.0), tight);
        assert!(b.width() <= 12.0 && b.center() == pos2(200.0, 50.0));
    }
}
