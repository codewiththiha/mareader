//! A pin where reading aloud starts: dragged between words, read on rest.

use std::sync::Arc;

use egui::{Color32, CursorIcon, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, vec2};
use mareader_pdf::PageText;
use mareader_session::{Anchor, TextLayer};
use mareader_speech::Spot;

use crate::selection::{self, Pages, Sheet};
use crate::theme::Tokens;
use mareader_engine::read_aloud::ReadFrom;

/// Seconds the pin rests before reading starts.
pub const REST: f64 = 1.2;
/// The pin's name, for screen readers and tests.
pub const LABEL: &str = "Reading starts here";
/// The knob's radius, and the bar's width under it.
const KNOB: f32 = 8.0;
const BAR_W: f32 = 3.0;
/// Room between the bar and its word.
const GAP: f32 = 3.0;
/// Seconds after a wheel turn that a held pin floats free of words.
const SETTLE: f64 = 0.15;
const APPEAR: f64 = 0.24;
const LEAVE: f64 = 0.3;
/// How fast a jump to a new word eases out, per second.
const GLIDE: f32 = 26.0;

/// The pin, from the page menu's Read aloud until reading starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    /// The word it sits on, at its first character.
    pub spot: Spot,
    /// Moved onto a word's start yet; not until its text has come.
    snapped: bool,
    born: Option<f64>,
    /// Reading starts `REST` after this; none while held.
    rest_from: Option<f64>,
    /// Held: the pointer's offset from the bar's middle.
    held: Option<Vec2>,
    wheeled: f64,
    /// Let go with the pointer still on it: the wait runs regardless.
    dropped: bool,
    /// Drawn this far from its place, easing to nothing.
    lag: Vec2,
    /// What it was last drawn on, and where its bar went.
    last: Option<(Place, Rect)>,
    leaving: Option<f64>,
    /// Brought into view once, when placed off screen.
    pub(crate) revealed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    Word(Spot),
    Free,
}

/// What the pin asks of its pane this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Stay,
    /// It rested, or was clicked: read from here.
    Read(ReadFrom),
    /// Its fade out is over.
    Gone,
}

impl Marker {
    /// A pin at `spot`, moved onto its word once drawn.
    pub fn new(spot: Spot) -> Self {
        Self {
            spot,
            snapped: false,
            born: None,
            rest_from: None,
            held: None,
            wheeled: f64::NEG_INFINITY,
            dropped: false,
            lag: Vec2::ZERO,
            last: None,
            leaving: None,
            revealed: false,
        }
    }

    /// Being dragged.
    pub fn held(&self) -> bool {
        self.held.is_some()
    }

    /// Fades out without reading; true if it was still waiting.
    pub fn dismiss(&mut self, now: f64) -> bool {
        self.held = None;
        let waiting = self.leaving.is_none();
        self.leaving.get_or_insert(now);
        waiting
    }

    pub fn leaving(&self) -> bool {
        self.leaving.is_some()
    }
}

/// The pin over `pages`: drawn, dragged, and counting down to read.
pub fn show(ui: &Ui, area: Rect, pages: &Pages<'_>, m: &mut Marker, t: &Tokens) -> Step {
    let (now, dt) = ui.input(|i| (i.time, i.stable_dt.clamp(0.0, 0.1)));
    let born = *m.born.get_or_insert(now);
    if let Some(since) = m.leaving {
        let k = ((now - since) / LEAVE).clamp(0.0, 1.0) as f32;
        if k >= 1.0 {
            return Step::Gone;
        }
        if let Some((_, bar)) = m.last {
            let look = Look {
                alpha: 1.0 - ease_out(k),
                swell: 1.0 + 0.5 * ease_out(k),
                ..Look::default()
            };
            paint(ui, bar, None, &look, t);
        }
        ui.ctx().request_repaint();
        return Step::Stay;
    }
    if !m.snapped
        && let Some((_, layer)) = shown(pages, m.spot.page)
        && let Some(char) = word_at(&layer.text, m.spot.char)
    {
        m.spot.char = char;
        m.snapped = true;
    }
    // Hit where it was drawn last frame, as egui's widgets are.
    let hit = m.last.map(|(_, bar)| reach(bar));
    let r = hit.map(|hit| {
        let r = ui.interact(hit, ui.id().with("read-pin"), Sense::click_and_drag());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, LABEL));
        r
    });
    let pointer = r.as_ref().and_then(egui::Response::interact_pointer_pos);
    // Held from where the press began, not where the drag was noticed.
    let origin = ui.input(|i| i.pointer.press_origin()).or(pointer);
    if let (Some(r), Some(p), Some((_, bar))) = (&r, origin, m.last)
        && r.drag_started()
    {
        m.held = Some(p - bar.center());
    }
    let dragging = r.as_ref().is_some_and(|r| r.dragged()) && m.held.is_some();
    let mut place = m.snapped.then_some(Place::Word(m.spot));
    let mut free_at = None;
    if dragging && let (Some(p), Some(off)) = (pointer, m.held) {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        if ui.input(|i| i.smooth_scroll_delta != Vec2::ZERO) {
            m.wheeled = now;
        }
        selection::wheel(ui, pages.sideways);
        selection::edge_scroll(ui, area, p);
        let hot = p - off;
        // Turning the wheel lets go of words until the page settles.
        let settled = now - m.wheeled > SETTLE;
        match word_under(pages, hot + vec2(GAP + BAR_W, 0.0), true).filter(|_| settled) {
            Some(spot) => {
                m.spot = spot;
                m.snapped = true;
                place = Some(Place::Word(spot));
            }
            None => {
                place = Some(Place::Free);
                free_at = Some(hot);
            }
        }
        m.rest_from = None;
    }
    let mut let_go = false;
    if let (Some(r), Some(off)) = (&r, m.held)
        && r.drag_stopped()
    {
        // Let go anywhere, it takes the nearest word.
        let hot = pointer
            .or_else(|| ui.input(|i| i.pointer.latest_pos()))
            .map(|p| p - off);
        if let Some(spot) = hot.and_then(|h| word_under(pages, h + vec2(GAP + BAR_W, 0.0), false)) {
            m.spot = spot;
            m.snapped = true;
        }
        m.held = None;
        m.dropped = true;
        let_go = true;
        m.rest_from = Some(now);
        place = m.snapped.then_some(Place::Word(m.spot));
    }
    let hovered = r.as_ref().is_some_and(egui::Response::hovered) && !dragging;
    // It stays let go until the pointer leaves it.
    m.dropped &= hovered || let_go;
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    if let Some(r) = &r
        && r.clicked()
    {
        m.leaving = Some(now);
        return Step::Read(read_from(pages, m.spot));
    }
    if let Some(r) = r.filter(|_| hovered) {
        r.on_hover_text("Drag to move it, or click to read now");
    }
    // Where it goes: on its word, or under the pointer while free.
    let word = match place {
        Some(Place::Word(spot)) => word_bar(pages, spot),
        _ => None,
    };
    let target = match (word, free_at) {
        (Some((bar, _)), _) => Some(bar),
        (None, Some(hot)) => {
            let h = m.last.map_or(20.0, |(_, b)| b.height());
            Some(Rect::from_center_size(hot, vec2(BAR_W, h)))
        }
        (None, None) => None,
    };
    let rest_from = *m.rest_from.get_or_insert(now);
    let Some((place, target)) = place.zip(target) else {
        // Its word is off screen or not read yet: the wait holds.
        m.rest_from = Some(rest_from + f64::from(dt));
        m.last = None;
        return Step::Stay;
    };
    // A jump to a new word glides there; a scrolled page carries it.
    if let Some((was, drawn)) = m.last
        && was != place
    {
        m.lag = drawn.center() - target.center();
    }
    m.lag *= (-GLIDE * dt).exp();
    if m.lag.length() < 0.3 {
        m.lag = Vec2::ZERO;
    }
    let h = match m.last {
        Some((_, drawn)) => {
            drawn.height() + (target.height() - drawn.height()) * (1.0 - (-GLIDE * dt).exp())
        }
        None => target.height(),
    };
    let bar = Rect::from_center_size(target.center() + m.lag, vec2(BAR_W, h));
    m.last = Some((place, bar));
    // Hovered, it waits for the grab it's about to get.
    if hovered && !m.dropped {
        m.rest_from = Some(rest_from + f64::from(dt));
    }
    let waited = m.rest_from.map_or(0.0, |from| now - from);
    if !dragging && waited >= REST {
        m.leaving = Some(now);
        return Step::Read(read_from(pages, m.spot));
    }
    let appear = ((now - born) / APPEAR).clamp(0.0, 1.0) as f32;
    let look = Look {
        alpha: appear.min(1.0),
        swell: ease_out_back(appear)
            * if dragging {
                1.25
            } else if hovered {
                1.12
            } else {
                1.0
            },
        ring: (!dragging).then_some((waited / REST).clamp(0.0, 1.0) as f32),
        held: dragging,
        free: place == Place::Free,
    };
    paint(ui, bar, word.map(|(_, w)| w), &look, t);
    ui.ctx().request_repaint();
    Step::Stay
}

/// How the pin is drawn this frame.
#[derive(Clone, Copy, Debug)]
struct Look {
    alpha: f32,
    /// The knob's scale: grown under the pointer, more while held.
    swell: f32,
    /// How far the wait to read has gone, 0 to 1; none while held.
    ring: Option<f32>,
    held: bool,
    free: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            swell: 1.0,
            ring: None,
            held: false,
            free: false,
        }
    }
}

/// The knob sits atop the bar like a pin's head.
fn knob(bar: Rect) -> Pos2 {
    bar.center_top() - vec2(0.0, KNOB - 2.0)
}

/// Where a press takes the pin: its knob, and its bar a little wider.
fn reach(bar: Rect) -> Rect {
    let head = Rect::from_center_size(knob(bar), Vec2::splat(2.0 * (KNOB + 6.0)));
    head.union(bar.expand2(vec2(6.0, 2.0)))
}

fn paint(ui: &Ui, bar: Rect, word: Option<Rect>, look: &Look, t: &Tokens) {
    let p = ui.painter();
    let a = look.alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let ink = t
        .accent
        .gamma_multiply(a * if look.free { 0.6 } else { 1.0 });
    // The word reading starts on, softly lit.
    if let Some(w) = word {
        p.rect_filled(
            w.expand2(vec2(2.0, 1.0)),
            3.0,
            t.accent.gamma_multiply(0.16 * a),
        );
    }
    let head = knob(bar);
    let r = KNOB * look.swell;
    if look.held {
        p.circle_filled(head, r * 1.9, t.accent.gamma_multiply(0.14 * a));
    }
    p.rect_filled(bar, BAR_W * 0.5, ink);
    p.circle_filled(head, r, ink);
    // A play mark in the knob says what resting here will do.
    let w = r * 0.42;
    let tip = head + vec2(w * 0.95, 0.0);
    let tri = vec![head + vec2(-w * 0.55, -w), head + vec2(-w * 0.55, w), tip];
    let light = Color32::WHITE.gamma_multiply(0.95 * a);
    p.add(Shape::convex_polygon(tri, light, Stroke::NONE));
    // The wait to read fills a ring round the knob.
    if let Some(done) = look.ring {
        let ring = r + 3.5;
        p.circle_stroke(
            head,
            ring,
            Stroke::new(2.0, t.accent.gamma_multiply(0.18 * a)),
        );
        if done > 0.0 {
            let n = (48.0 * done).ceil().max(2.0) as usize;
            let arc: Vec<Pos2> = (0..=n)
                .map(|i| {
                    let turn = done * i as f32 / n as f32;
                    let angle = std::f32::consts::TAU * turn - std::f32::consts::FRAC_PI_2;
                    head + ring * Vec2::angled(angle)
                })
                .collect();
            p.add(Shape::line(arc, Stroke::new(2.0, ink)));
        }
    }
}

/// Starts read from `spot`: its place in the book too, where it has one.
fn read_from(pages: &Pages<'_>, spot: Spot) -> ReadFrom {
    let layer = pages.texts.get(pages.doc, spot.page);
    let place = layer.and_then(|l| l.place(spot.char));
    ReadFrom {
        anchor: place.map(|(block, offset)| Anchor { block, offset }),
        at: Some(spot),
        until: None,
    }
}

/// `page` on screen, and its text once read.
fn shown(pages: &Pages<'_>, page: usize) -> Option<(Sheet, Arc<TextLayer>)> {
    let sheet = pages
        .sheets
        .iter()
        .find(|s| s.page == page && s.shown.is_positive())?;
    Some((*sheet, pages.texts.get(pages.doc, page)?))
}

/// The word at or nearest point `p`; `strict` wants `p` on a line.
fn word_under(pages: &Pages<'_>, p: Pos2, strict: bool) -> Option<Spot> {
    let sheet = pages.sheets.iter().find(|s| s.shown.contains(p))?;
    let layer = pages.texts.get(pages.doc, sheet.page)?;
    let (x, y) = sheet.to_page(p);
    if strict && !layer.text.on_line(x, y) {
        return None;
    }
    // The letter under it, else the caret nearest.
    let i = layer
        .text
        .char_at(x, y)
        .unwrap_or_else(|| layer.text.caret_at(x, y));
    let char = word_at(&layer.text, i)?;
    Some(Spot {
        page: sheet.page,
        char,
    })
}

/// The bar beside `spot`'s word, the height of its line, and the word.
fn word_bar(pages: &Pages<'_>, spot: Spot) -> Option<(Rect, Rect)> {
    let (sheet, layer) = shown(pages, spot.page)?;
    let text = &layer.text;
    let chars = text.chars();
    let line = chars.get(spot.char)?.line;
    let end = word_end(text, spot.char);
    let on_line = |i: &usize| chars.get(*i).is_some_and(|c| c.line == line);
    let first = (0..spot.char)
        .rev()
        .take_while(on_line)
        .last()
        .unwrap_or(spot.char);
    let last = (spot.char..chars.len())
        .take_while(on_line)
        .last()
        .unwrap_or(spot.char);
    let word = sheet.to_screen(text.line_boxes(spot.char..end).next()?);
    let row = sheet.to_screen(text.line_boxes(first..last + 1).next()?);
    let x = word.left() - GAP - BAR_W * 0.5;
    let bar = Rect::from_x_y_ranges(
        x - BAR_W * 0.5..=x + BAR_W * 0.5,
        row.top() - 2.0..=row.bottom() + 2.0,
    );
    Some((bar, word))
}

fn wordy(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '\'' | '\u{2019}')
}

/// A word begins at `i`: after a gap, a line break or a non-letter.
fn starts(text: &PageText, i: usize) -> bool {
    let chars = text.chars();
    let Some(c) = chars.get(i).filter(|c| wordy(c.ch)) else {
        return false;
    };
    match i.checked_sub(1).and_then(|j| chars.get(j)) {
        None => true,
        Some(b) => !wordy(b.ch) || c.space_before || b.line != c.line,
    }
}

/// The start of the word holding `i`, else the next one, else the last.
fn word_at(text: &PageText, i: usize) -> Option<usize> {
    let len = text.len();
    let i = i.min(len.saturating_sub(1));
    let inside = text.chars().get(i).is_some_and(|c| wordy(c.ch));
    if inside {
        return (0..=i).rev().find(|&j| starts(text, j));
    }
    (i..len)
        .find(|&j| starts(text, j))
        .or_else(|| (0..i).rev().find(|&j| starts(text, j)))
}

/// The warm tone of the word reading would start on.
pub(crate) fn warm(t: &Tokens) -> Color32 {
    match t.dark {
        true => Color32::from_rgb(255, 190, 84),
        false => Color32::from_rgb(240, 150, 20),
    }
}

/// The word at or nearest character `i`: the one a pin would take.
pub(crate) fn word_span(text: &PageText, i: usize) -> Option<std::ops::Range<usize>> {
    let start = word_at(text, i)?;
    Some(start..word_end(text, start))
}

/// Where the word starting at `start` ends.
fn word_end(text: &PageText, start: usize) -> usize {
    let len = text.len();
    (start + 1..len)
        .find(|&j| starts(text, j) || !text.chars().get(j).is_some_and(|c| wordy(c.ch)))
        .unwrap_or(len)
}

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

/// Overshoots a touch, then settles: the pin drops into place.
fn ease_out_back(x: f32) -> f32 {
    let (c1, c3) = (1.70158_f32, 2.70158_f32);
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_lands_where_it_should() {
        assert!(ease_out_back(0.0).abs() < 1e-6);
        assert!((ease_out_back(1.0) - 1.0).abs() < 1e-6);
        assert!(ease_out_back(0.7) > 1.0, "it overshoots");
        assert!((ease_out(1.0) - 1.0).abs() < 1e-6);
    }
}
