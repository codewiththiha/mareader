//! Scratch-built 2D colour wheel and rotary dials for the Dial deck popover.

use std::ops::RangeInclusive;

use egui::{
    Align2, Color32, CursorIcon, FontId, Id, Key, Mesh, Painter, Pos2, Rect, Response, Sense,
    Shape, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_appearance::{Appearance, FULL_TINT};
use mareader_engine::appearance_menu::{STRENGTH_ON_HUE, strength_percent};

use crate::theme::{self, Tokens};
use crate::widgets;

/// Named landmarks around mareader's hue circle, in clockwise order.
pub const HUES: [(u16, &str); 7] = [
    (14, "Rose"),
    (34, "Sepia"),
    (104, "Green"),
    (160, "Mint"),
    (200, "Sky"),
    (240, "Blue"),
    (290, "Violet"),
];

pub const WHEEL_SIZE: f32 = 132.0;
const R_DISC: f32 = 45.0;
const R_RING_OUT: f32 = 51.5;
const R_TRAVEL: f32 = 36.5;
const R_LANDMARK: f32 = 58.5;
const LANDMARK_HIT: f32 = 12.0;
const SNAP_DEG: f32 = 6.0;
const DIAL_FROM: f32 = -135.0;
const DIAL_TO: f32 = 135.0;
const DRAG_TRAVEL_PX: f32 = 135.0;

/// How a rotary dial scales, steps, resets and formats its number.
pub struct KnobSpec<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub range: RangeInclusive<f32>,
    pub step: f32,
    pub default: f32,
    pub enabled: bool,
    pub shown: String,
}

/// Landmark name when `hue` sits on one, else `None`.
pub fn landmark_name(hue: u16) -> Option<&'static str> {
    HUES.iter()
        .find(|(deg, _)| *deg == hue % 360)
        .map(|(_, name)| *name)
}

/// Vivid sRGB swatch matching mareader's tint hue circle (HSL 62%, 55%).
pub fn hue_colour(deg: f32) -> Color32 {
    let (s, l) = (0.62_f32, 0.55_f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = deg.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c * 0.5;
    let ch = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(ch(r), ch(g), ch(b))
}

pub(super) fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let k = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| {
        let v = f32::from(x) + (f32::from(y) - f32::from(x)) * k;
        v.round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// Point at `deg` clockwise from 12 o'clock on radius `r`.
fn polar(c: Pos2, r: f32, deg: f32) -> Pos2 {
    let rad = (deg - 90.0).to_radians();
    pos2(c.x + r * rad.cos(), c.y + r * rad.sin())
}

/// Clockwise angle in `0.0..360.0` from 12 o'clock.
fn angle_from_center(delta: Vec2) -> f32 {
    (delta.y.atan2(delta.x).to_degrees() + 90.0).rem_euclid(360.0)
}

fn angular_distance(a: f32, b: f32) -> f32 {
    let diff = (a - b).rem_euclid(360.0);
    if diff > 180.0 { 360.0 - diff } else { diff }
}

/// Maps a wheel pointer offset to `(tint_hue, tint_strength)`.
fn pointer_to_tint(delta: Vec2, fallback_hue: u16) -> (u16, u8) {
    let dist = delta.length();
    if dist < 3.5 {
        return (fallback_hue % 360, 0);
    }
    let mut deg = angle_from_center(delta);
    if dist >= R_TRAVEL * 0.65 {
        let near = HUES
            .iter()
            .copied()
            .find(|(h, _)| angular_distance(deg, f32::from(*h)) <= SNAP_DEG);
        if let Some((h, _)) = near {
            deg = f32::from(h);
        }
    }
    let hue = (deg.round() as u16) % 360;
    let norm = (dist / R_TRAVEL).clamp(0.0, 1.0);
    let strength = (norm * f32::from(FULL_TINT)).round() as u8;
    (hue, strength.min(FULL_TINT))
}

/// 2D hue-and-strength wheel with rim landmarks; returns true when edited.
pub fn color_wheel(ui: &mut Ui, a: &mut Appearance) -> bool {
    let t = Tokens::get(ui.ctx());
    let (box_rect, _) = ui.allocate_exact_size(Vec2::splat(WHEEL_SIZE), Sense::hover());
    let c = box_rect.center();
    let wheel_id = ui.id().with("dial-deck-color-wheel");
    let wheel_rect = Rect::from_center_size(c, Vec2::splat(R_RING_OUT * 2.0 + 2.0));
    let mut resp = ui.interact(wheel_rect, wheel_id, Sense::click_and_drag());
    let mut changed = false;

    if resp.double_clicked() && a.tint_strength != 0 {
        a.tint_strength = 0;
        changed = true;
    } else if (resp.dragged() || resp.clicked())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let delta = p - c;
        let outside_ring = !resp.dragged() && delta.length() > R_RING_OUT + 2.0;
        if !outside_ring {
            let (hue, strength) = pointer_to_tint(delta, a.tint_hue);
            if (hue, strength) != (a.tint_hue, a.tint_strength) {
                a.tint_hue = hue;
                a.tint_strength = strength;
                changed = true;
            }
        }
    }

    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() >= 1.0 {
            let step: i16 = if scroll > 0.0 { 1 } else { -1 };
            let next = (i16::from(a.tint_strength) + step).clamp(0, i16::from(FULL_TINT)) as u8;
            if next != a.tint_strength {
                a.tint_strength = next;
                changed = true;
            }
        }
    }

    if resp.has_focus() {
        changed |= wheel_keys(ui, a);
    }

    let pct = f64::from(strength_percent(a.tint_strength));
    resp.widget_info(|| WidgetInfo::slider(true, pct, "Tint colour and strength"));
    if changed {
        resp.mark_changed();
    }

    let painter = ui.painter();
    paint_wheel_disc(painter, c, a, &t);
    paint_hue_ring(painter, c);
    if resp.has_focus() {
        painter.circle_stroke(c, R_RING_OUT + 2.0, Stroke::new(1.5, t.accent));
    }

    for (deg, name) in HUES {
        let dot_c = polar(c, R_LANDMARK, f32::from(deg));
        let dot_rect = Rect::from_center_size(dot_c, Vec2::splat(LANDMARK_HIT));
        let dot_id = wheel_id.with(("landmark", deg));
        let dot_resp = ui.interact(dot_rect, dot_id, Sense::click());
        let active = a.has_tint() && a.tint_hue == deg;
        let hot = dot_resp.hovered();
        if hot {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if dot_resp.clicked() {
            a.tint_hue = deg;
            if a.tint_strength == 0 {
                a.tint_strength = STRENGTH_ON_HUE;
            }
            changed = true;
        }
        paint_landmark(painter, dot_c, deg, active, hot, &t);
        let a11y_label = format!("{name} hue");
        dot_resp
            .widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, &a11y_label));
        let tip = format!("{name} ({deg}°)");
        widgets::tip(&dot_resp, &tip, None);
    }

    paint_puck(painter, c, a, resp.hovered() || resp.dragged(), &t);
    changed
}

fn wheel_keys(ui: &Ui, a: &mut Appearance) -> bool {
    let fine = ui.input(|i| i.modifiers.shift);
    let hue_step: i32 = if fine { 1 } else { 5 };
    let mut edited = false;
    ui.input(|i| {
        if i.key_pressed(Key::ArrowRight) {
            a.tint_hue = (i32::from(a.tint_hue) + hue_step).rem_euclid(360) as u16;
            if a.tint_strength == 0 {
                a.tint_strength = STRENGTH_ON_HUE;
            }
            edited = true;
        } else if i.key_pressed(Key::ArrowLeft) {
            a.tint_hue = (i32::from(a.tint_hue) - hue_step).rem_euclid(360) as u16;
            if a.tint_strength == 0 {
                a.tint_strength = STRENGTH_ON_HUE;
            }
            edited = true;
        } else if i.key_pressed(Key::ArrowUp) {
            let next = (a.tint_strength + 1).min(FULL_TINT);
            edited = next != a.tint_strength;
            a.tint_strength = next;
        } else if i.key_pressed(Key::ArrowDown) {
            let next = a.tint_strength.saturating_sub(1);
            edited = next != a.tint_strength;
            a.tint_strength = next;
        } else if i.key_pressed(Key::Home)
            || i.key_pressed(Key::Backspace)
            || i.key_pressed(Key::Delete)
        {
            edited = a.tint_strength != 0;
            a.tint_strength = 0;
        }
    });
    edited
}

fn paint_wheel_disc(painter: &Painter, c: Pos2, a: &Appearance, t: &Tokens) {
    const SEGMENTS: usize = 72;
    const STOPS: [f32; 5] = [0.0, 0.34, 0.62, 0.85, 1.0];
    let mut mesh = Mesh::default();
    let base = t.paper;
    let max_mix = if t.dark { 0.56 } else { 0.72 };
    let active_share = if a.has_tint() {
        (f32::from(a.tint_strength) / 14.0).clamp(0.45, 1.0)
    } else {
        0.0
    };
    let chosen = hue_colour(f32::from(a.tint_hue));

    let center_idx = mesh.vertices.len() as u32;
    mesh.colored_vertex(c, base);

    let ring_count = STOPS.len() - 1;
    for &stop in STOPS.iter().skip(1) {
        let r = R_DISC * stop;
        let bloom = ((stop - 0.30) / 0.70).clamp(0.0, 1.0);
        let curved = bloom * bloom * (3.0 - 2.0 * bloom);
        for s in 0..SEGMENTS {
            let deg = (s as f32 / SEGMENTS as f32) * 360.0;
            let ambient = hue_colour(deg);
            let target = blend(ambient, chosen, active_share);
            let strength = if a.has_tint() {
                curved * max_mix
            } else {
                curved * (if t.dark { 0.20 } else { 0.24 })
            };
            let col = blend(base, target, strength);
            mesh.colored_vertex(polar(c, r, deg), col);
        }
    }

    let n = SEGMENTS as u32;
    for s in 0..n {
        let next = (s + 1) % n;
        mesh.add_triangle(center_idx, 1 + s, 1 + next);
    }
    for ring in 0..(ring_count.saturating_sub(1) as u32) {
        let inner = 1 + ring * n;
        let outer = inner + n;
        for s in 0..n {
            let next = (s + 1) % n;
            mesh.add_triangle(inner + s, outer + s, outer + next);
            mesh.add_triangle(inner + s, outer + next, inner + next);
        }
    }
    painter.add(mesh);

    let seam = if t.dark {
        Color32::from_white_alpha(36)
    } else {
        Color32::from_white_alpha(150)
    };
    painter.circle_stroke(c, R_DISC, Stroke::new(1.0, seam));
}

fn paint_hue_ring(painter: &Painter, c: Pos2) {
    const SEGMENTS: usize = 96;
    const AA: f32 = 0.85;
    let mut mesh = Mesh::default();
    let radii = [
        (R_DISC - AA, 0_u8),
        (R_DISC, 255_u8),
        (R_RING_OUT, 255_u8),
        (R_RING_OUT + AA, 0_u8),
    ];

    for s in 0..SEGMENTS {
        let deg = (s as f32 / SEGMENTS as f32) * 360.0;
        let rgb = hue_colour(deg);
        for &(r, alpha) in &radii {
            let col = Color32::from_rgba_unmultiplied(rgb.r(), rgb.g(), rgb.b(), alpha);
            mesh.colored_vertex(polar(c, r, deg), col);
        }
    }

    let n = SEGMENTS as u32;
    for s in 0..n {
        let next = (s + 1) % n;
        let b0 = s * 4;
        let b1 = next * 4;
        for band in 0..3_u32 {
            mesh.add_triangle(b0 + band, b0 + band + 1, b1 + band + 1);
            mesh.add_triangle(b0 + band, b1 + band + 1, b1 + band);
        }
    }
    painter.add(mesh);
}

fn paint_landmark(painter: &Painter, c: Pos2, deg: u16, active: bool, hot: bool, t: &Tokens) {
    let outer = if active || hot { 5.4 } else { 4.6 };
    let core = if active || hot { 3.5 } else { 2.9 };
    painter.circle_filled(
        c + vec2(0.0, 0.8),
        outer + 0.7,
        Color32::from_black_alpha(48),
    );
    painter.circle_filled(c, outer, Color32::WHITE);
    painter.circle_filled(c, core, hue_colour(f32::from(deg)));
    if active {
        painter.circle_stroke(c, outer + 1.4, Stroke::new(1.4, t.accent));
    }
}

fn paint_puck(painter: &Painter, c: Pos2, a: &Appearance, hot: bool, t: &Tokens) {
    let share = f32::from(a.tint_strength.min(FULL_TINT)) / f32::from(FULL_TINT);
    let puck = polar(c, share * R_TRAVEL, f32::from(a.tint_hue));
    let outer = if hot { 8.8 } else { 8.2 };
    painter.circle_filled(
        puck + vec2(0.0, 1.2),
        outer + 1.2,
        Color32::from_black_alpha(60),
    );
    painter.circle_filled(puck, outer, Color32::WHITE);
    painter.circle_stroke(puck, outer, Stroke::new(1.0, Color32::from_black_alpha(55)));
    let fill = if a.has_tint() {
        hue_colour(f32::from(a.tint_hue))
    } else {
        blend(t.paper, t.muted, 0.35)
    };
    painter.circle_filled(puck, outer - 2.8, fill);
}

/// Vertical rotary dial column: circular knob above centred label and value.
/// `dial_px` is the dial diameter, so a tab with spare room can dial it up.
pub fn rotary_knob(
    ui: &mut Ui,
    width: f32,
    dial_px: f32,
    spec: &KnobSpec<'_>,
    value: &mut f32,
) -> bool {
    let t = Tokens::get(ui.ctx());
    // The 34pt caption ends flush at any dial size.
    let total_h = dial_px + 34.0;
    let (col_rect, _) = ui.allocate_exact_size(vec2(width, total_h), Sense::hover());
    let dial_c = pos2(col_rect.center().x, col_rect.top() + dial_px * 0.5);
    let dial_rect = Rect::from_center_size(dial_c, Vec2::splat(dial_px));
    let changed = interact_and_paint_dial(ui, dial_rect, spec, value, &t);

    let painter = ui.painter_at(col_rect);
    let label_col = if spec.enabled { t.ink } else { t.muted };
    let val_col = if spec.enabled {
        t.muted
    } else {
        t.muted.gamma_multiply(0.6)
    };
    painter.text(
        pos2(col_rect.center().x, col_rect.bottom() - 26.0),
        Align2::CENTER_CENTER,
        spec.label,
        theme::medium(11.5),
        label_col,
    );
    painter.text(
        pos2(col_rect.center().x, col_rect.bottom() - 12.0),
        Align2::CENTER_CENTER,
        &spec.shown,
        FontId::proportional(11.0),
        val_col,
    );
    changed
}

/// Horizontal dial with label and value stacked at its right.
pub fn inline_knob(ui: &mut Ui, width: f32, spec: &KnobSpec<'_>, value: &mut f32) -> bool {
    let t = Tokens::get(ui.ctx());
    const DIAL_PX: f32 = 48.0;
    let (row_rect, _) = ui.allocate_exact_size(vec2(width, DIAL_PX), Sense::hover());
    let dial_c = pos2(row_rect.left() + DIAL_PX * 0.5, row_rect.center().y);
    let dial_rect = Rect::from_center_size(dial_c, Vec2::splat(DIAL_PX));
    let changed = interact_and_paint_dial(ui, dial_rect, spec, value, &t);

    let text_x = dial_rect.right() + 10.0;
    let painter = ui.painter_at(row_rect);
    let label_col = if spec.enabled { t.ink } else { t.muted };
    painter.text(
        pos2(text_x, row_rect.center().y - 7.5),
        Align2::LEFT_CENTER,
        spec.label,
        theme::medium(12.0),
        label_col,
    );
    painter.text(
        pos2(text_x, row_rect.center().y + 8.5),
        Align2::LEFT_CENTER,
        &spec.shown,
        FontId::proportional(11.5),
        t.muted,
    );
    changed
}

fn snap_step(raw: f32, lo: f32, hi: f32, step: f32) -> f32 {
    if step <= 0.0 {
        return raw.clamp(lo, hi);
    }
    let steps = ((raw - lo) / step).round();
    let snapped = lo + steps * step;
    ((snapped * 1000.0).round() / 1000.0).clamp(lo, hi)
}

fn interact_and_paint_dial(
    ui: &mut Ui,
    rect: Rect,
    spec: &KnobSpec<'_>,
    value: &mut f32,
    t: &Tokens,
) -> bool {
    let lo = *spec.range.start();
    let hi = *spec.range.end();
    let span = (hi - lo).max(f32::EPSILON);
    let id = ui.id().with(("dial-deck-knob", spec.id));
    let drag_mem = Id::new("dial-deck-knob-drag").with(id);
    let sense = if spec.enabled {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let mut resp = ui.interact(rect, id, sense);
    let mut changed = false;

    if spec.enabled {
        if resp.double_clicked() {
            let next = snap_step(spec.default, lo, hi, spec.step);
            if (next - *value).abs() > 1e-4 {
                *value = next;
                changed = true;
            }
            ui.data_mut(|d| d.remove::<f32>(drag_mem));
        } else if resp.drag_started() {
            ui.data_mut(|d| d.insert_temp(drag_mem, *value));
        }

        if resp.dragged() {
            let delta = resp.drag_delta();
            let travel = (-delta.y + delta.x * 0.5) / DRAG_TRAVEL_PX;
            let raw = ui.data(|d| d.get_temp::<f32>(drag_mem)).unwrap_or(*value);
            let next_raw = (raw + travel * span).clamp(lo, hi);
            ui.data_mut(|d| d.insert_temp(drag_mem, next_raw));
            let snapped = snap_step(next_raw, lo, hi, spec.step);
            if (snapped - *value).abs() > 1e-4 {
                *value = snapped;
                changed = true;
            }
        } else if resp.drag_stopped() {
            ui.data_mut(|d| d.remove::<f32>(drag_mem));
        }

        if resp.hovered() {
            ui.ctx().set_cursor_icon(if resp.dragged() {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() >= 1.0 {
                let dir = if scroll > 0.0 { 1.0 } else { -1.0 };
                let next = snap_step(*value + dir * spec.step, lo, hi, spec.step);
                if (next - *value).abs() > 1e-4 {
                    *value = next;
                    changed = true;
                }
            }
        }

        if resp.has_focus() {
            changed |= knob_keys(ui, spec, value, lo, hi);
        }
    }

    let current = f64::from(*value);
    let label = spec.label;
    resp.widget_info(|| WidgetInfo::slider(spec.enabled, current, label));
    if changed {
        resp.mark_changed();
    }
    if spec.enabled {
        let tip = format!("{}: {} (double-click to reset)", spec.label, spec.shown);
        widgets::tip(&resp, &tip, None);
    }

    paint_dial_face(ui.painter(), rect, *value, (lo, hi), spec.enabled, &resp, t);
    changed
}

fn knob_keys(ui: &Ui, spec: &KnobSpec<'_>, value: &mut f32, lo: f32, hi: f32) -> bool {
    let s = spec.step.max(0.01);
    let mut target = None;
    ui.input(|i| {
        if i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::ArrowRight) {
            target = Some(*value + s);
        } else if i.key_pressed(Key::ArrowDown) || i.key_pressed(Key::ArrowLeft) {
            target = Some(*value - s);
        } else if i.key_pressed(Key::PageUp) {
            target = Some(*value + s * 5.0);
        } else if i.key_pressed(Key::PageDown) {
            target = Some(*value - s * 5.0);
        } else if i.key_pressed(Key::Home) {
            target = Some(lo);
        } else if i.key_pressed(Key::End) {
            target = Some(hi);
        }
    });
    if let Some(raw) = target {
        let next = snap_step(raw, lo, hi, spec.step);
        if (next - *value).abs() > 1e-4 {
            *value = next;
            return true;
        }
    }
    false
}

fn paint_dial_face(
    painter: &Painter,
    rect: Rect,
    value: f32,
    (lo, hi): (f32, f32),
    enabled: bool,
    resp: &Response,
    t: &Tokens,
) {
    let c = rect.center();
    let half = rect.width() * 0.5;
    let r_arc = half - 4.5;
    let r_cap = half - 10.5;
    let stroke_w = 4.6;

    // The unfilled sweep must read as part of the dial, not as dead space.
    let track_col = blend(t.paper, t.ink, if t.dark { 0.26 } else { 0.17 });
    paint_arc(painter, c, r_arc, DIAL_FROM, DIAL_TO, stroke_w, track_col);

    let frac = ((value - lo) / (hi - lo).max(f32::EPSILON)).clamp(0.0, 1.0);
    let angle = DIAL_FROM + frac * (DIAL_TO - DIAL_FROM);
    // Disabled dials omit value marks that would imply zero.
    if enabled {
        if frac > 0.005 {
            paint_arc(painter, c, r_arc, DIAL_FROM, angle, stroke_w, t.accent);
        } else {
            let start = polar(c, r_arc, DIAL_FROM);
            painter.circle_filled(start, stroke_w * 0.5, t.accent);
        }
    }

    let hot = enabled && (resp.hovered() || resp.dragged());
    if enabled {
        painter.circle_filled(c + vec2(0.0, 1.0), r_cap, Color32::from_black_alpha(26));
    }
    let cap_fill = blend(
        t.paper,
        t.ink,
        match (t.dark, hot) {
            (true, true) => 0.11,
            (true, false) => 0.06,
            (false, true) => 0.05,
            (false, false) => 0.02,
        },
    );
    let cap_border = if resp.dragged() || resp.has_focus() {
        Stroke::new(1.2, t.accent)
    } else if hot {
        Stroke::new(1.0, blend(t.line, t.ink, 0.22))
    } else {
        Stroke::new(1.0, t.line)
    };
    painter.circle(c, r_cap, cap_fill, cap_border);

    if enabled {
        let p0 = polar(c, r_cap * 0.24, angle);
        let p1 = polar(c, r_cap * 0.80, angle);
        let needle_w = 2.4;
        painter.line_segment([p0, p1], Stroke::new(needle_w, t.accent));
        painter.circle_filled(p0, needle_w * 0.5, t.accent);
        painter.circle_filled(p1, needle_w * 0.5, t.accent);
    }
}

fn paint_arc(
    painter: &Painter,
    c: Pos2,
    r: f32,
    from_deg: f32,
    to_deg: f32,
    width: f32,
    color: Color32,
) {
    let span = (to_deg - from_deg).abs();
    if span < 0.4 {
        painter.circle_filled(polar(c, r, from_deg), width * 0.5, color);
        return;
    }
    let steps = ((span / 5.0).ceil() as usize).max(2);
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let deg = from_deg + (to_deg - from_deg) * (i as f32 / steps as f32);
        pts.push(polar(c, r, deg));
    }
    if let (Some(&first), Some(&last)) = (pts.first(), pts.last()) {
        painter.add(Shape::line(pts, Stroke::new(width, color)));
        painter.circle_filled(first, width * 0.5, color);
        painter.circle_filled(last, width * 0.5, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_geometry_maps_angles_and_snaps_near_landmarks() {
        assert_eq!(pointer_to_tint(Vec2::ZERO, 120), (120, 0));
        let top = vec2(0.0, -R_TRAVEL);
        assert_eq!(pointer_to_tint(top, 0), (0, FULL_TINT));
        let right = vec2(R_TRAVEL * 0.5, 0.0);
        assert_eq!(pointer_to_tint(right, 0), (90, FULL_TINT / 2));
        // Near Sepia (34°) on the outer third, gentle landmark snap kicks in.
        let near_sepia = polar(Pos2::ZERO, R_TRAVEL * 0.9, 36.5).to_vec2();
        assert_eq!(pointer_to_tint(near_sepia, 0).0, 34);
    }

    #[test]
    fn knob_step_snapping_respects_bounds_and_increments() {
        assert!((snap_step(102.3, 50.0, 200.0, 5.0) - 100.0).abs() < 1e-4);
        assert!((snap_step(103.0, 50.0, 200.0, 5.0) - 105.0).abs() < 1e-4);
        assert!((snap_step(999.0, 50.0, 200.0, 5.0) - 200.0).abs() < 1e-4);
        assert!((snap_step(1.74, 1.0, 3.0, 0.1) - 1.7).abs() < 1e-4);
    }
}
