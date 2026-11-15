//! One toast's card: measured first, so a stack can place it, then drawn.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{
    Button, Color32, Context, CornerRadius, CursorIcon, FontId, Galley, Mesh, Painter, Rect,
    Stroke, StrokeKind, Ui, Vec2, pos2,
};

use super::Live;
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;
use mareader_engine::toast::Tone;

const PAD_X: f32 = 14.0;
/// Fixed card height so one-line and two-line toasts share a steady silhouette.
const HEIGHT: f32 = 52.0;
/// The tone's round mark at the left.
const MARK: f32 = 16.0;
const GAP: f32 = 10.0;
const CLOSE: f32 = 24.0;
const ACTION_H: f32 = 28.0;
/// How soft the countdown's edge is, in points.
const FEATHER: f32 = 64.0;
/// The countdown's columns are this far apart, in points.
const COLUMN: f32 = 3.0;

/// A card laid out for one width.
pub(super) struct Card {
    galley: Arc<Galley>,
    pub height: f32,
    /// The action button's width, nothing without one.
    action: f32,
}

fn action_job(ctx: &Context, live: &Live, t: &Tokens) -> LayoutJob {
    let mut job = LayoutJob::default();
    let med = theme::medium(12.5);
    let label_font = if ctx.fonts_mut(|f| f.families().contains(&med.family)) {
        med
    } else {
        FontId::proportional(12.5)
    };
    job.append(
        live.toast.label,
        0.0,
        TextFormat::simple(label_font, t.accent),
    );
    if live.toast.label == "Undo" {
        let platform = ctx
            .data(|d| d.get_temp::<crate::platform::Platform>(egui::Id::new("toast-platform")))
            .unwrap_or_else(crate::platform::Platform::current);
        let key = if platform == crate::platform::Platform::Mac {
            "  ⌘Z"
        } else {
            "  Ctrl+Z"
        };
        job.append(
            key,
            0.0,
            TextFormat::simple(FontId::proportional(10.5), t.muted),
        );
    }
    job
}

pub(super) fn measure(ctx: &Context, live: &Live, width: f32, t: &Tokens) -> Card {
    let font = FontId::proportional(t.text);
    let action = match live.toast.undo {
        Some(_) => {
            let job = action_job(ctx, live, t);
            let galley = ctx.fonts_mut(|f| f.layout_job(job));
            galley.size().x + 20.0
        }
        None => 0.0,
    };
    let wrap = width - 2.0 * PAD_X - MARK - GAP - action - CLOSE - 8.0;
    let mut job = LayoutJob::default();
    job.append(&live.toast.message, 0.0, TextFormat::simple(font, t.ink));
    if live.times > 1 {
        let count = format!("  ×{}", live.times);
        let small = FontId::proportional(t.text_small);
        job.append(&count, 0.0, TextFormat::simple(small, t.muted));
    }
    job.wrap.max_width = wrap.max(80.0);
    job.wrap.max_rows = 2;
    let galley = ctx.fonts_mut(|f| f.layout_job(job));
    Card {
        galley,
        height: HEIGHT,
        action,
    }
}

/// The tone's icon, its colour and the card's edge.
fn look(tone: Tone, t: &Tokens) -> (Icon, Color32, Color32) {
    match tone {
        Tone::Info => (Icon::Done, t.accent, t.line),
        Tone::Error => (Icon::Alert, t.danger, t.danger.gamma_multiply(0.5)),
        Tone::Warning => {
            let amber = crate::theme::caution(t);
            (Icon::Alert, amber, amber.gamma_multiply(0.5))
        }
    }
}

/// How a card's body is drawn where it sits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Body {
    /// In front, floating over the page.
    Front,
    /// Peeking out behind: a shade toward the ink, so it reads on any page.
    Behind,
    /// In the opened list, flat on the panel.
    Listed,
}

/// The card's body alone, as a toast behind the front one shows.
pub(super) fn shell(ui: &Ui, rect: Rect, tone: Tone, body: Body, t: &Tokens) {
    let (_, _, edge) = look(tone, t);
    let radius = CornerRadius::from(t.radius_lg);
    let painter = ui.painter();
    let fill = match body {
        Body::Front => t.surface,
        Body::Behind => t.surface.lerp_to_gamma(t.ink, 0.07),
        Body::Listed => t.card,
    };
    if body == Body::Front {
        painter.add(ui.style().visuals.popup_shadow.as_shape(rect, radius));
    }
    let stroke = Stroke::new(1.0, edge);
    painter.rect(rect, radius, fill, stroke, StrokeKind::Inside);
}

/// The close button's place on a card at `rect`, centred like the mark.
fn close_at(rect: Rect) -> Rect {
    let row = rect.center().y;
    let x = rect.right() - 10.0 - CLOSE / 2.0;
    Rect::from_center_size(pos2(x, row), Vec2::splat(CLOSE))
}

/// The action button's place, left of the close button.
fn action_at(rect: Rect, card: &Card) -> Rect {
    let close = close_at(rect);
    let row = close.center().y;
    Rect::from_min_max(
        pos2(close.left() - 6.0 - card.action, row - ACTION_H / 2.0),
        pos2(close.left() - 6.0, row + ACTION_H / 2.0),
    )
}

/// Where a card's buttons sit: presses there never drag the card.
pub(super) fn controls(rect: Rect, card: &Card) -> Rect {
    let close = close_at(rect);
    match card.action > 0.0 {
        true => close.union(action_at(rect, card)),
        false => close,
    }
}

/// The time left as a wash of the tone, its edge feathered away.
fn countdown(painter: &Painter, rect: Rect, radius: f32, tint: Color32, left: f32) {
    let edge = rect.left() - FEATHER / 2.0 + (rect.width() + FEATHER) * left;
    let end = (edge + FEATHER / 2.0).min(rect.right());
    if end <= rect.left() {
        return;
    }
    // Eased both ways, so the edge has no visible start or stop.
    let alpha = |x: f32| {
        let p = ((edge + FEATHER / 2.0 - x) / FEATHER).clamp(0.0, 1.0);
        p * p * p * (p * (p * 6.0 - 15.0) + 10.0)
    };
    let r = radius.min(rect.height() / 2.0).min(rect.width() / 2.0);
    // The card's top and bottom at `x`, its corners rounded.
    let span = |x: f32| {
        let dx = (rect.left() + r - x).max(x - (rect.right() - r)).max(0.0);
        let inset = r - (r * r - dx * dx).max(0.0).sqrt();
        (rect.top() + inset, rect.bottom() - inset)
    };
    let mut mesh = Mesh::default();
    let mut x = rect.left();
    loop {
        let (top, bottom) = span(x);
        let colour = tint.gamma_multiply(alpha(x));
        let at = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(x, top), colour);
        mesh.colored_vertex(pos2(x, bottom), colour);
        if at >= 2 {
            mesh.add_triangle(at - 2, at - 1, at);
            mesh.add_triangle(at - 1, at, at + 1);
        }
        if x >= end {
            break;
        }
        x = (x + COLUMN).min(end);
    }
    painter.add(mesh);
}

/// The wash's strength for a tone: a hint of colour, never a block.
fn wash(tone: Tone, tint: Color32) -> Color32 {
    match tone {
        Tone::Error => tint.gamma_multiply(0.16),
        Tone::Info | Tone::Warning => tint.gamma_multiply(0.13),
    }
}

/// The time left, the mark, the words and the buttons.
pub(super) fn content(ui: &mut Ui, rect: Rect, live: &Live, card: &Card, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let (icon, tint, _) = look(live.toast.tone, &t);
    let painter = ui.painter().clone();
    let left = live.remaining();
    if left > 0.0 && !live.listed {
        let inner = rect.shrink(1.0);
        let radius = t.radius_lg - 1.0;
        countdown(&painter, inner, radius, wash(live.toast.tone, tint), left);
    }
    // Mark, words and buttons share the card's middle, however many lines.
    let middle = rect.center().y;
    let mark = Rect::from_center_size(
        pos2(rect.left() + PAD_X + MARK / 2.0, middle),
        Vec2::splat(MARK),
    );
    icon.paint(
        ui,
        Rect::from_center_size(mark.center(), Vec2::splat(MARK)),
        tint,
    );
    let words = card.galley.size().y;
    let text = pos2(mark.right() + GAP, (middle - words / 2.0).round());
    painter.galley(text, card.galley.clone(), t.ink);
    if live.going() {
        return;
    }
    let dismiss = |ui: &mut Ui| widgets::small_icon_button(ui, Icon::Close, "Dismiss");
    let dismissed = ui.put(close_at(rect), dismiss);
    if dismissed
        .on_hover_cursor(CursorIcon::PointingHand)
        .clicked()
    {
        out.push(Sent::Ui(UiCommand::CloseToast(live.id)));
    }
    let Some(command) = &live.toast.undo else {
        return;
    };
    let at = action_at(rect, card);
    let button = Button::new(action_job(ui.ctx(), live, &t))
        .fill(t.accent_soft)
        .stroke(Stroke::NONE)
        .corner_radius(8);
    let pressed = ui.put(at, button).on_hover_cursor(CursorIcon::PointingHand);
    pressed.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, live.toast.label)
    });
    if pressed.clicked() {
        out.push(Sent::Engine((**command).clone()));
        out.push(Sent::Ui(UiCommand::CloseToast(live.id)));
    }
}
