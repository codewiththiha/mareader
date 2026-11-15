//! Drag-to-tuck rail hint and parked edge tab for the Dictionary window.

use egui::{
    Align2, Context, FontId, Id, Order, Rect, RichText, Sense, Stroke, Ui, Vec2, WidgetInfo,
    WidgetType, epaint::TextShape, pos2, vec2,
};

use crate::icons::Icon;
use crate::theme::Tokens;

use super::page::Page;

/// Paints the edge-tuck guide rail while dragging past a stage edge.
pub(super) fn drag_hint(ctx: &Context, stage: Rect, left: bool, ready: bool) {
    let t = Tokens::get(ctx);
    let x = if left {
        stage.left() + 4.0
    } else {
        stage.right() - 10.0
    };
    let rail = Rect::from_min_max(
        pos2(x, stage.top() + 12.0),
        pos2(x + 6.0, (stage.bottom() - 12.0).max(stage.top() + 12.0)),
    );
    let painter = ctx.layer_painter(egui::LayerId::new(
        Order::Foreground,
        Id::new("dict-edge-hint"),
    ));
    painter.rect_filled(
        rail,
        3.0,
        t.accent.gamma_multiply(if ready { 0.9 } else { 0.5 }),
    );
    let text = if ready {
        "Release to tuck"
    } else {
        "Keep going to tuck"
    };
    let at = if left {
        pos2(rail.right() + 8.0, rail.center().y)
    } else {
        pos2(rail.left() - 8.0, rail.center().y)
    };
    let align = if left {
        Align2::LEFT_CENTER
    } else {
        Align2::RIGHT_CENTER
    };
    painter.text(at, align, text, FontId::proportional(12.0), t.ink);
}

/// Paints the parked edge tab and hover peek popup; returns its rect and click.
pub(super) fn edge_tab(
    ctx: &Context,
    edge: crate::park::Edge,
    shown: Rect,
    stage: Rect,
    page: &Page,
) -> (Rect, bool) {
    let t = Tokens::get(ctx);
    let word = page.text().trim();
    let label = if word.is_empty() { "Dictionary" } else { word };
    let short: String = label.chars().take(30).collect();
    let text =
        ctx.fonts_mut(|fonts| fonts.layout_no_wrap(short, FontId::proportional(12.0), t.ink));
    let size = vec2(
        30.0_f32.min(stage.width().max(0.0)),
        (text.size().x + 68.0).min(stage.height().max(0.0)),
    );
    let x = if edge == crate::park::Edge::Left {
        stage.left()
    } else {
        stage.right() - size.x
    };
    let y = shown
        .top()
        .clamp(stage.top(), (stage.bottom() - size.y).max(stage.top()));
    let rect = Rect::from_min_size(pos2(x, y), size);
    if size.x <= 0.0 || size.y <= 0.0 {
        return (rect, false);
    }
    let response = egui::Area::new(Id::new("dict-edge-tab"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false)
        .movable(false)
        .show(ctx, |ui| {
            paint_edge_tab(
                ui,
                (edge, rect, stage),
                (label, &text, page.entry_count()),
                &t,
            )
        })
        .inner;
    edge_tab_peek(
        ctx,
        (edge, rect, stage),
        (label, page.glance()),
        response.hovered(),
        &t,
    );
    (rect, response.clicked())
}

fn paint_edge_tab(
    ui: &mut Ui,
    (edge, rect, stage): (crate::park::Edge, Rect, Rect),
    (label, text, count): (&str, &std::sync::Arc<egui::Galley>, usize),
    t: &Tokens,
) -> egui::Response {
    ui.set_clip_rect(rect.intersect(stage));
    let hit = ui.interact(rect, Id::new("dict-edge-tab-button"), Sense::click());
    hit.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            true,
            format!("Restore dictionary: {label}"),
        )
    });
    let fill = if hit.hovered() { t.card } else { t.surface };
    ui.painter().rect_filled(rect, t.radius_lg, fill);
    ui.painter().rect_stroke(
        rect,
        t.radius_lg,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    let icon = Rect::from_center_size(pos2(rect.center().x, rect.top() + 17.0), Vec2::splat(15.0));
    Icon::Dictionary.paint(ui, icon, t.accent);
    let text_pos = if edge == crate::park::Edge::Left {
        pos2(
            rect.center().x - text.size().y / 2.0,
            rect.top() + 27.0 + text.size().x,
        )
    } else {
        pos2(rect.center().x + text.size().y / 2.0, rect.top() + 27.0)
    };
    let angle = if edge == crate::park::Edge::Left {
        -std::f32::consts::FRAC_PI_2
    } else {
        std::f32::consts::FRAC_PI_2
    };
    ui.painter()
        .add(TextShape::new(text_pos, text.clone(), t.ink).with_angle(angle));
    if count > 0 {
        let c = pos2(rect.center().x, rect.bottom() - 28.0);
        ui.painter().circle_filled(c, 9.0, t.accent_soft);
        ui.painter().text(
            c,
            Align2::CENTER_CENTER,
            count.to_string(),
            FontId::proportional(10.0),
            t.accent,
        );
    }
    let arrow = if edge == crate::park::Edge::Left {
        Icon::ChevronRight
    } else {
        Icon::ChevronLeft
    };
    arrow.paint(
        ui,
        Rect::from_center_size(
            pos2(rect.center().x, rect.bottom() - 12.0),
            Vec2::splat(13.0),
        ),
        t.muted,
    );
    hit
}

fn edge_tab_peek(
    ctx: &Context,
    (edge, rect, stage): (crate::park::Edge, Rect, Rect),
    (label, glance): (&str, Option<&str>),
    hovered: bool,
    t: &Tokens,
) {
    let id = Id::new("dict-edge-peek-start");
    let now = ctx.input(|i| i.time);
    let since = ctx.data_mut(|data| {
        if hovered {
            let start = data.get_temp::<f64>(id).unwrap_or(now);
            data.insert_temp(id, start);
            Some(start)
        } else {
            data.remove_temp::<f64>(id);
            None
        }
    });
    let Some(since) = since else {
        return;
    };
    let wait = (since + 0.15 - now).max(0.0);
    if wait > 0.0 {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        return;
    }
    let x = if edge == crate::park::Edge::Left {
        rect.right() + 6.0
    } else {
        rect.left() - 216.0
    };
    egui::Area::new(Id::new("dict-edge-peek"))
        .order(Order::Tooltip)
        .fixed_pos(pos2(
            x.clamp(stage.left(), (stage.right() - 210.0).max(stage.left())),
            rect.top(),
        ))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(210.0_f32.min(stage.width().max(0.0)));
                ui.label(
                    RichText::new("Peek · click the tab to restore")
                        .small()
                        .color(t.muted),
                );
                ui.label(label);
                if let Some(glance) = glance {
                    ui.label(RichText::new(glance).color(t.accent));
                }
            });
        });
}
