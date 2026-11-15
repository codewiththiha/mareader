//! The question reading asks at a part it may pass over, as a table.

use std::sync::Arc;

use egui::{
    Align2, Context, CornerRadius, CursorIcon, Galley, Id, Order, Rect, Sense, Shadow, Stroke, Ui,
    WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_engine::read_aloud::parts::Kind;

use crate::icons::Icon;
use crate::motion;
use crate::theme::{self, Tokens};

/// The pill's height, and its gap above the player.
const HEIGHT: f32 = 44.0;
const FOOT_H: f32 = 28.0;
const GAP: f32 = 10.0;
const PAD: f32 = 7.0;
/// The icon's disc, and the answers' height.
const DISC: f32 = 30.0;
const BUTTON_H: f32 = 30.0;
/// How far it rises as it comes in.
const RISE: f32 = 8.0;

/// Asks about `part` over `player`, `shown` of the way in; the answer.
pub(super) fn show(
    ctx: &Context,
    (player, room): (Rect, Rect),
    part: Kind,
    cost: Option<&str>,
    shown: f32,
    remember: Option<&mut bool>,
) -> Option<bool> {
    let foot = pos2(player.center().x, player.top() - GAP + (1.0 - shown) * RISE);
    let area = egui::Area::new(Id::new("read-aloud-ask"))
        .fade_in(false)
        .order(Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(foot)
        .constrain(false)
        .interactable(shown > 0.9);
    let asked = area.show(ctx, |ui| {
        ui.set_opacity(shown);
        ui.set_clip_rect(room);
        pill(ui, part, cost, room.width(), remember)
    });
    crate::toast::keep_clear(ctx, asked.response.rect);
    asked.inner
}

/// The pill: the part's icon and name, then Read and Skip.
fn pill(
    ui: &mut Ui,
    part: Kind,
    cost: Option<&str>,
    room: f32,
    remember: Option<&mut bool>,
) -> Option<bool> {
    let t = Tokens::get(ui.ctx());
    let painter = ui.painter().clone();
    let name = part.name();
    let lower = name.to_lowercase();
    let title = painter.layout_no_wrap(format!("{name} ahead"), theme::medium(t.text), t.ink);
    let small = theme::medium(t.text_small);
    let compact = room < 350.0;
    let read_label = if compact { "Read" } else { "Read  Space" };
    let skip_label = if compact { "Skip" } else { "Skip  Esc" };
    let read = painter.layout_no_wrap(read_label.to_owned(), small.clone(), t.ink);
    let skip = painter.layout_no_wrap(skip_label.to_owned(), small, theme::ink_on(t.accent));
    let cost = cost.map(|cost| {
        painter.layout_no_wrap(cost.to_owned(), egui::FontId::proportional(10.5), t.muted)
    });
    let rem_label = format!("Always skip {lower}s in this book");
    let rem_width = if remember.is_some() {
        let font = egui::FontId::proportional(t.text_small);
        let g = painter.layout_no_wrap(rem_label.clone(), font, t.muted);
        g.size().x + 44.0
    } else {
        0.0
    };
    let wide = |g: &Galley| g.size().x + 26.0;
    let cost_w = cost.as_ref().map_or(0.0, |c| c.size().x);
    let text_w = title.size().x.max(cost_w);
    let top_w = PAD + DISC + 10.0 + text_w + 18.0 + wide(&read) + 6.0 + wide(&skip) + PAD;
    let width = top_w.max(rem_width).min(room.max(200.0));
    let height = if remember.is_some() {
        HEIGHT + FOOT_H
    } else {
        HEIGHT
    };
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let round = CornerRadius::same(16);
    let base = ui.ctx().global_style().visuals.window_shadow;
    painter.add(base.as_shape(rect, round));
    painter.rect_filled(rect, round, t.surface);
    let edge = Stroke::new(1.0, t.line);
    painter.rect_stroke(rect, round, edge, egui::StrokeKind::Inside);
    let y = rect.top() + HEIGHT / 2.0;
    let disc = pos2(rect.left() + PAD + DISC / 2.0, y);
    painter.rect_filled(
        Rect::from_center_size(disc, egui::Vec2::splat(DISC)),
        9.0,
        t.accent_soft,
    );
    let icon = Rect::from_center_size(disc, egui::Vec2::splat(16.0));
    sign(part).image(ui, 16.0).tint(t.accent).paint_at(ui, icon);
    let x = disc.x + DISC / 2.0 + 10.0;
    let top = if cost.is_some() {
        y - 15.0
    } else {
        y - title.size().y / 2.0
    };
    let title_height = title.size().y;
    painter.galley(pos2(x, top), title, t.ink);
    if let Some(cost) = cost {
        painter.galley(pos2(x, top + title_height), cost, t.muted);
    }
    let mut right = rect.right() - PAD;
    let mut slot = |g: &Galley| {
        let r = Rect::from_min_max(
            pos2(right - wide(g), y - BUTTON_H / 2.0),
            pos2(right, y + BUTTON_H / 2.0),
        );
        right = r.left() - 6.0;
        r
    };
    let (skip_at, read_at) = (slot(&skip), slot(&read));
    let mut answer = None;
    if button(ui, skip_at, skip, &format!("Skip the {lower}"), true) {
        answer = Some(false);
    }
    if button(ui, read_at, read, &format!("Read the {lower}"), false) {
        answer = Some(true);
    }
    if let Some(remember) = remember {
        let rule_y = rect.top() + HEIGHT;
        painter.hline(
            rect.left() + 12.0..=rect.right() - 12.0,
            rule_y,
            Stroke::new(1.0, t.line.gamma_multiply(0.65)),
        );
        let foot = Rect::from_min_max(
            pos2(rect.left() + PAD, rule_y),
            pos2(rect.right() - PAD, rect.bottom()),
        );
        let align = egui::Align::Center;
        let base_layout = egui::Layout::left_to_right(align);
        let layout = base_layout.with_main_align(align);
        let builder = egui::UiBuilder::new().max_rect(foot).layout(layout);
        ui.scope_builder(builder, |ui| {
            let w = &mut ui.visuals_mut().widgets;
            let stroke = Stroke::new(1.0, t.muted.gamma_multiply(0.45));
            for s in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                s.corner_radius = CornerRadius::same(4);
                s.bg_stroke = stroke;
            }
            if *remember {
                w.inactive.bg_fill = t.accent;
                w.inactive.bg_stroke = Stroke::NONE;
                w.inactive.fg_stroke = Stroke::new(1.5, theme::ink_on(t.accent));
            }
            ui.add_space(4.0);
            let text = egui::RichText::new(rem_label).size(t.text_small);
            ui.checkbox(remember, text.color(t.muted));
        });
    }
    answer
}

/// One answer: filled in the accent if `primary`, else quiet.
fn button(ui: &mut Ui, rect: Rect, text: Arc<Galley>, label: &str, primary: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let r = ui.interact(rect, ui.id().with(("ask", label)), Sense::click());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let ctx = ui.ctx().clone();
    let quick = motion::secs(&ctx, 0.1);
    let hot = ctx.animate_bool_with_time(r.id.with("hot"), r.hovered(), quick);
    let down = ctx.animate_bool_with_time(r.id.with("down"), r.is_pointer_button_down_on(), quick);
    if r.hovered() {
        ctx.set_cursor_icon(CursorIcon::PointingHand);
    }
    let shape = rect.shrink(down * 1.5);
    let round = CornerRadius::same(10);
    let painter = ui.painter();
    if primary {
        let fill = t.accent.gamma_multiply(1.0 - 0.12 * hot);
        let lift = Shadow {
            offset: [0, 1],
            blur: (4.0 + 4.0 * hot) as u8,
            spread: 0,
            color: t.accent.gamma_multiply(0.35),
        };
        painter.add(lift.as_shape(shape, round));
        painter.rect_filled(shape, round, fill);
    } else {
        painter.rect_filled(shape, round, t.hover.gamma_multiply(0.6 + 0.4 * hot));
    }
    let at = shape.center() - text.size() / 2.0;
    let ink = if primary {
        theme::ink_on(t.accent)
    } else {
        t.ink
    };
    painter.galley(at, text, ink);
    r.clicked()
}

/// The glyph a part of the book shows by.
pub(super) fn sign(kind: Kind) -> Icon {
    match kind {
        Kind::Table => Icon::Table,
        Kind::List => Icon::List,
        Kind::Figure => Icon::Figure,
        Kind::Aside => Icon::Aside,
    }
}
