//! The segmented control, drawn with egui: a soft well whose raised thumb
//! slides to the pick, with thin seams between the other choices.

use egui::{Color32, CornerRadius, FontId, Id, Rect, Response, Sense, Shadow, Stroke, Ui, Vec2};
use egui::{pos2, vec2};

use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::theme::{self, Tokens};

/// A track's row height where none is asked: compact, for menus.
const SEGMENT: f32 = 24.0;
/// The well's rim around the thumb.
const INSET: f32 = 2.0;
/// Room beside a choice's words, each side, when a track fits its words.
const PAD: f32 = 14.0;
/// An icon's room before its words.
const ICON_W: f32 = 18.0;

/// Choices as one segmented track, `cols` equal cells to a row.
pub fn segments<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    all: &[T],
    cols: usize,
    face: impl Fn(T) -> (Option<Icon>, &'static str),
) {
    segments_of(ui, SEGMENT, value, all, cols, face);
}

/// The width a one-row track needs so its widest choice's words fit.
pub fn segments_width(ui: &Ui, labels: &[&str]) -> f32 {
    let t = Tokens::get(ui.ctx());
    let font = theme::medium(t.text);
    let widest = labels
        .iter()
        .map(|l| {
            ui.painter()
                .layout_no_wrap((*l).to_owned(), font.clone(), t.ink)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    (widest + PAD * 2.0) * labels.len().max(1) as f32 + INSET * 2.0
}

/// Segments with rows `row` tall, to line up with taller controls.
pub fn segments_of<T: Copy + PartialEq>(
    ui: &mut Ui,
    row: f32,
    value: &mut T,
    all: &[T],
    cols: usize,
    face: impl Fn(T) -> (Option<Icon>, &'static str),
) {
    let t = Tokens::get(ui.ctx());
    // A segment animates on selection because its track stays visible.
    let _act = anim::announce(ui, Act::Click);
    let cols = cols.max(1);
    let rows = all.len().div_ceil(cols);
    let size = vec2(ui.available_width(), row * rows as f32);
    let (track, track_resp) = ui.allocate_exact_size(size, Sense::hover());
    let grid = Grid {
        track,
        cols,
        row,
        n: all.len(),
    };
    let base = track_resp.id;
    ui.painter().rect_filled(track, grid.round(0.0), well(&t));
    // The thumb slides to a new pick instead of jumping.
    let thumb = all.iter().position(|v| v == value).map(|i| {
        let ctx = ui.ctx();
        let x = ctx.animate_value_with_time(base.with("thumb-x"), (i % cols) as f32, 0.16);
        let y = ctx.animate_value_with_time(base.with("thumb-y"), (i / cols) as f32, 0.16);
        (x, y)
    });
    seams(ui, &grid, thumb, &t);
    if let Some((x, y)) = thumb {
        raised(ui, &grid, grid.cell(x, y), &t);
    }
    for (i, &v) in all.iter().enumerate() {
        let on = *value == v;
        if choice(ui, (&grid, base), i, face(v), on).clicked() {
            *value = v;
        }
    }
}

/// Where a track's cells sit.
struct Grid {
    track: Rect,
    cols: usize,
    row: f32,
    n: usize,
}

impl Grid {
    fn width(&self) -> f32 {
        self.track.width() / self.cols as f32
    }

    /// The cell at column `x`, row `y`, inside the well's rim; fractions
    /// land between cells, for the sliding thumb.
    fn cell(&self, x: f32, y: f32) -> Rect {
        let min = self.track.min + vec2(x * self.width(), y * self.row);
        Rect::from_min_size(min, vec2(self.width(), self.row)).shrink(INSET)
    }

    /// The well's corners, or a thumb's `inset` inside them.
    fn round(&self, inset: f32) -> CornerRadius {
        let r = (self.row * 0.3).min(9.0) - inset;
        CornerRadius::same(r.max(2.0) as u8)
    }
}

/// The well: a faint wash of ink over whatever the track sits on.
fn well(t: &Tokens) -> Color32 {
    t.ink.gamma_multiply(if t.dark { 0.09 } else { 0.065 })
}

/// The thumb: lifted off the well; white in light looks, a brighter wash
/// of ink in dark ones, so it reads on any ground.
fn raised(ui: &Ui, grid: &Grid, cell: Rect, t: &Tokens) {
    let round = grid.round(INSET);
    let (fill, rim) = match t.dark {
        true => (t.ink.gamma_multiply(0.17), t.ink.gamma_multiply(0.1)),
        false => (
            t.surface.lerp_to_gamma(Color32::WHITE, 0.85),
            t.line.gamma_multiply(0.7),
        ),
    };
    let shadow = Shadow {
        offset: [0, 1],
        blur: 3,
        spread: 0,
        color: t.shadow.gamma_multiply(0.55),
    };
    let painter = ui.painter();
    painter.add(shadow.as_shape(cell, round));
    painter.rect_filled(cell, round, fill);
    painter.rect_stroke(cell, round, Stroke::new(1.0, rim), egui::StrokeKind::Inside);
}

/// Hairlines between neighbours, fading where the thumb is near.
fn seams(ui: &Ui, grid: &Grid, thumb: Option<(f32, f32)>, t: &Tokens) {
    let half = grid.row * 0.26;
    for i in 1..grid.n {
        let (x, y) = ((i % grid.cols) as f32, (i / grid.cols) as f32);
        if x == 0.0 {
            continue;
        }
        let near = thumb
            .filter(|(_, ty)| (ty - y).abs() < 1.0)
            .map_or(1.0, |(tx, _)| {
                ((tx - x + 1.0).abs()).min((tx - x).abs()).clamp(0.0, 1.0)
            });
        let at = grid.track.left() + x * grid.width();
        let mid = grid.track.top() + (y + 0.5) * grid.row;
        let ink = t.line.gamma_multiply(near);
        ui.painter()
            .vline(at, (mid - half)..=(mid + half), Stroke::new(1.0, ink));
    }
}

/// The `i`th choice's words (and icon) over its cell; its response.
fn choice(
    ui: &mut Ui,
    (grid, base): (&Grid, Id),
    i: usize,
    (icon, label): (Option<Icon>, &'static str),
    on: bool,
) -> Response {
    let t = Tokens::get(ui.ctx());
    let cell = grid.cell((i % grid.cols) as f32, (i / grid.cols) as f32);
    let resp = ui.interact(cell, base.with(("segment", i, label)), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if !on && (resp.hovered() || resp.is_pointer_button_down_on()) {
        let wash = if resp.is_pointer_button_down_on() {
            0.08
        } else {
            0.04
        };
        (ui.painter()).rect_filled(cell, grid.round(INSET), t.ink.gamma_multiply(wash));
    }
    let ink = if on || resp.hovered() { t.ink } else { t.muted };
    let font = if on {
        theme::medium(t.text)
    } else {
        FontId::proportional(t.text)
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
    let icon_w = if icon.is_some() { ICON_W } else { 0.0 };
    let mut x = cell.center().x - (icon_w + galley.size().x) / 2.0;
    let y = cell.center().y;
    if let Some(icon) = icon {
        let at = Rect::from_min_size(pos2(x, y - 7.0), Vec2::splat(14.0));
        // Stable cell names preserve runs when earlier controls change.
        ui.push_id(("segment-icon", base, label), |ui| {
            anim::paint(ui, at, icon, ink, label, &resp);
        });
        x += icon_w;
    }
    let top = y - galley.size().y / 2.0;
    ui.painter().galley(pos2(x, top), galley, ink);
    let info = || egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label);
    resp.widget_info(info);
    resp
}
