//! Chrome, header bar, and resize handles for the Dictionary window.

use egui::{
    Align, Context, CursorIcon, Id, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2,
    vec2,
};
use mareader_engine::command::Command;
use mareader_engine::dict::{Action, Window};

use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;
use crate::z_order::{self, Tier};

use super::window::{CLOSE, EXPAND, Frame, MINIMIZE, RESTORE};
use super::window_geom::{GRIP, HEAD, Morph, ease, stretch};

/// Per-frame paint parameters for the Dictionary window area.
pub(super) struct AreaState {
    pub shown: Rect,
    pub appear: f32,
    pub closing: bool,
    pub folded: bool,
    pub parked: bool,
    pub tucked_rest: bool,
}

/// Paints the Dictionary floating area; returns its layer and release flag.
pub(super) fn paint_window_area(
    ctx: &Context,
    f: &mut Frame<'_>,
    rect: &mut Rect,
    morph: &Morph,
    state: &AreaState,
    t: &Tokens,
    out: &mut Commands,
) -> (egui::LayerId, bool) {
    let mut released = false;
    let area = z_order::area(ctx, Tier::FloatingTool, Id::new("dict-window"))
        .fixed_pos(state.shown.min)
        .constrain(false)
        .movable(false)
        .interactable(!state.closing && !state.parked);
    let layer = area.layer();
    let grow = 0.96 + 0.04 * state.appear;
    let anchor = pos2(state.shown.center().x, state.shown.top());
    let lift = vec2(0.0, 8.0 * (1.0 - state.appear));
    let scale = egui::emath::TSTransform::from_translation(anchor.to_vec2() + lift)
        * egui::emath::TSTransform::from_scaling(grow)
        * egui::emath::TSTransform::from_translation(-anchor.to_vec2());
    ctx.set_transform_layer(layer, scale);
    area.show(ctx, |ui| {
        released = paint_area_contents(ui, f, rect, morph, state, t, out);
    });
    (layer, released)
}

fn paint_area_contents(
    ui: &mut Ui,
    f: &mut Frame<'_>,
    rect: &mut Rect,
    morph: &Morph,
    state: &AreaState,
    t: &Tokens,
    out: &mut Commands,
) -> bool {
    let shown = state.shown;
    ui.multiply_opacity(state.appear);
    ui.set_clip_rect(f.stage);
    if state.closing {
        ui.disable();
    }
    ui.set_min_size(shown.size());
    ui.set_max_size(shown.size());
    if state.tucked_rest && f.parking == mareader_settings::EdgeParking::Tab {
        let _ = ui.allocate_exact_size(shown.size(), Sense::hover());
        return false;
    }
    paint(ui, shown, morph.fold, t);
    let head = Rect::from_min_size(shown.min, vec2(shown.width(), HEAD));
    let mut ignored = Vec::new();
    let target = if state.parked { &mut ignored } else { out };
    let (grab_dragged, mut released) =
        paint_header_bar(ui, f, rect, (head, morph, state), t, target);
    if state.folded {
        f.page.ask(ui.ctx(), f.session, f.settings);
    }
    let body_shown = (1.0 - 2.0 * morph.fold).max(0.0);
    if body_shown <= 0.0 {
        return released;
    }
    ui.multiply_opacity(body_shown);
    paint_body(ui, f, (shown, head, morph.open), t, target);
    let (edges_released, edges_dragging) = if morph.fold <= 0.0 && !state.closing && !state.parked {
        edges(ui, shown, rect)
    } else {
        (false, false)
    };
    released |= edges_released;
    if grab_dragged || edges_dragging {
        *f.dragging = true;
        *f.evacuated = None;
        if f.toasts
            .is_some_and(|toast| shown.intersects(toast) || rect.intersects(toast))
        {
            target.push(Sent::Ui(UiCommand::ClearToasts));
        }
    }
    released
}

fn paint_header_bar(
    ui: &mut Ui,
    f: &Frame<'_>,
    rect: &mut Rect,
    (head, morph, state): (Rect, &Morph, &AreaState),
    t: &Tokens,
    out: &mut Commands,
) -> (bool, bool) {
    let grab = ui.interact(head, Id::new("dict-window-head"), Sense::click_and_drag());
    if grab.dragged() {
        *rect = rect.translate(grab.drag_delta());
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    if state.folded && grab.clicked() {
        out.push(Sent::Engine(Command::Dict(Action::Window(
            Window::Minimize(false),
        ))));
    } else if !state.folded && grab.double_clicked() {
        out.push(Sent::Engine(Command::Dict(Action::Window(
            Window::Minimize(true),
        ))));
    }
    let named = Named {
        word: f.page.text().trim(),
        glance: f.page.glance().filter(|_| morph.fold > 0.5),
    };
    if state.folded {
        grab.clone().on_hover_text("Click to restore");
    }
    let head_shown = (2.0 * morph.fold - 1.0).abs();
    ui.scope(|ui| {
        ui.multiply_opacity(head_shown);
        header(ui, head, morph.fold > 0.5, &named, t, out);
    });
    (grab.dragged(), grab.drag_stopped())
}

fn paint_body(
    ui: &mut Ui,
    f: &mut Frame<'_>,
    (shown, head, open): (Rect, Rect, Rect),
    t: &Tokens,
    out: &mut Commands,
) {
    let line = Stroke::new(1.0, t.line);
    ui.painter().hline(shown.x_range(), head.bottom(), line);
    let body = Rect::from_min_max(
        pos2(shown.left() + 12.0, head.bottom() + 10.0),
        pos2(
            shown.left() + open.width() - 12.0,
            shown.top() + open.height() - 10.0,
        ),
    );
    let cut = Rect::from_min_max(head.left_bottom(), shown.max).shrink(1.0);
    let builder = UiBuilder::new()
        .max_rect(body)
        .layout(Layout::top_down(Align::Min));
    ui.scope_builder(builder, |ui| {
        ui.set_clip_rect(body.expand(2.0).intersect(cut).intersect(f.stage));
        f.page.compact(ui, f.session, (f.settings, f.voice), out);
    });
}

/// Shadow, paper and hairline, rounding as it folds into the pill.
fn paint(ui: &Ui, r: Rect, fold: f32, t: &Tokens) {
    let radius = egui::emath::lerp(t.radius_lg..=HEAD / 2.0, ease(fold));
    let shadow = egui::Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: t.shadow,
    };
    let painter = ui.painter();
    painter.add(shadow.as_shape(r, radius));
    painter.rect(
        r,
        radius,
        t.paper,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
}

/// What the folded pill names: the word, and its top answer at a glance.
struct Named<'a> {
    word: &'a str,
    glance: Option<&'a str>,
}

/// Icon and title, then open as a page, fold and close.
fn header(ui: &mut Ui, head: Rect, folded: bool, n: &Named<'_>, t: &Tokens, out: &mut Commands) {
    let row = head.shrink2(vec2(10.0, 0.0));
    let builder = UiBuilder::new()
        .max_rect(row)
        .layout(Layout::right_to_left(Align::Center));
    ui.scope_builder(builder, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        // The title is part of the handle, not text to select.
        ui.style_mut().interaction.selectable_labels = false;
        let send = |w: Window| Sent::Engine(Command::Dict(Action::Window(w)));
        if widgets::small_icon_button(ui, Icon::Close, CLOSE).clicked() {
            out.push(send(Window::Close));
        }
        let (icon, label) = match folded {
            true => (Icon::ChevronUp, RESTORE),
            false => (Icon::Minus, MINIMIZE),
        };
        if widgets::small_icon_button(ui, icon, label).clicked() {
            out.push(send(Window::Minimize(!folded)));
        }
        if !folded && widgets::small_icon_button(ui, Icon::Fullscreen, EXPAND).clicked() {
            out.push(send(Window::Expand));
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add(Icon::Dictionary.image(ui, 16.0).tint(t.accent));
            // Folded, it names the word last searched and its answer.
            let title = match folded && !n.word.is_empty() {
                true => n.word,
                false => "Dictionary",
            };
            let title = RichText::new(title).font(crate::theme::medium(t.text));
            let Some(glance) = n.glance else {
                crate::shaped::line(ui, title, t.text, t.ink);
                return;
            };
            // The word keeps at most half, the answer the rest, on one line.
            let half = vec2(
                (ui.available_width() * 0.5).max(40.0),
                ui.available_height(),
            );
            let lead = Layout::left_to_right(Align::Center);
            ui.allocate_ui_with_layout(half, lead, |ui| {
                crate::shaped::line(ui, title, t.text, t.ink);
            });
            crate::shaped::line(ui, glance, t.text_small, t.muted);
        });
    });
}

/// The eight resize bands; returns (any_released, any_dragging).
fn edges(ui: &Ui, r: Rect, rect: &mut Rect) -> (bool, bool) {
    let mut released = false;
    let mut dragging = false;
    let sides = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
        (1, 1),
    ];
    for (i, (dx, dy)) in sides.into_iter().enumerate() {
        let zone = band(r, dx, dy);
        let resp = ui.interact(zone, Id::new(("dict-window-edge", i)), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(cursor(dx, dy));
        }
        if resp.dragged() {
            *rect = stretch(*rect, dx, dy, resp.drag_delta());
            dragging = true;
        }
        released |= resp.drag_stopped();
    }
    (released, dragging)
}

/// The strip inside `r` along the sides `dx` and `dy` name.
fn band(r: Rect, dx: i8, dy: i8) -> Rect {
    let span = |d: i8, lo: f32, hi: f32| match d {
        -1 => (lo, lo + GRIP),
        1 => (hi - GRIP, hi),
        _ => (lo + GRIP, hi - GRIP),
    };
    let (x0, x1) = span(dx, r.left(), r.right());
    let (y0, y1) = span(dy, r.top(), r.bottom());
    Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
}

fn cursor(dx: i8, dy: i8) -> CursorIcon {
    match (dx, dy) {
        (-1, -1) | (1, 1) => CursorIcon::ResizeNwSe,
        (1, -1) | (-1, 1) => CursorIcon::ResizeNeSw,
        (_, 0) => CursorIcon::ResizeHorizontal,
        _ => CursorIcon::ResizeVertical,
    }
}
