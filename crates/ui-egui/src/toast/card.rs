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
/// From the card's left edge to its words: the tone's colour says the rest.
const LEAD: f32 = 16.0;
const CLOSE: f32 = 24.0;
/// The player's tab: its height and its words' inset.
pub(super) const ISLAND_H: f32 = 28.0;
pub(super) const ISLAND_PAD: f32 = 16.0;
const ACTION_H: f32 = 26.0;
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

/// The medium face at `size`, the regular one where it is missing.
pub(crate) fn medium(ctx: &Context, size: f32) -> FontId {
    let med = theme::medium(size);
    match ctx.fonts_mut(|f| f.families().contains(&med.family)) {
        true => med,
        false => FontId::proportional(size),
    }
}

fn action_job(ctx: &Context, live: &Live, mark: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let label_font = medium(ctx, 12.5);
    job.append(live.toast.label, 0.0, TextFormat::simple(label_font, mark));
    if live.toast.label == "Undo" {
        let platform = ctx
            .data(|d| d.get_temp::<crate::platform::Platform>(egui::Id::new("toast-platform")))
            .unwrap_or_else(crate::platform::Platform::current);
        let key = if platform == crate::platform::Platform::Mac {
            "  ⌘Z"
        } else {
            "  Ctrl+Z"
        };
        let hint = mark.gamma_multiply(0.65);
        job.append(
            key,
            0.0,
            TextFormat::simple(FontId::proportional(10.5), hint),
        );
    }
    job
}

pub(super) fn measure(ctx: &Context, live: &Live, width: f32, t: &Tokens) -> Card {
    let font = FontId::proportional(t.text);
    let swatch = look(live.toast.tone, t);
    let action = match live.toast.undo {
        Some(_) => {
            let job = action_job(ctx, live, swatch.mark);
            let galley = ctx.fonts_mut(|f| f.layout_job(job));
            galley.size().x + 22.0
        }
        None => 0.0,
    };
    let wrap = width - LEAD - PAD_X - action - CLOSE - 8.0;
    let mut job = LayoutJob::default();
    job.append(&live.toast.message, 0.0, TextFormat::simple(font, t.ink));
    if live.times > 1 {
        // In the tone, so a repeat reads at a glance.
        let count = format!("  ×{}", live.times);
        let small = medium(ctx, t.text_small);
        job.append(&count, 0.0, TextFormat::simple(small, swatch.mark));
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

/// Theme-inflected semantic colours for one toast card.
pub(super) struct Swatch {
    pub mark: Color32,
    pub listed: Color32,
    pub edge: Color32,
    pub soft: Color32,
    pub block: Color32,
}

/// The tone's theme-harmonised semantic swatch: green, amber or red.
pub(super) fn look(tone: Tone, t: &Tokens) -> Swatch {
    let raw = match tone {
        Tone::Info => t.toast.info,
        Tone::Warning => t.toast.warning,
        Tone::Error => t.toast.error,
    };
    Swatch {
        mark: theme::color(raw.mark),
        listed: theme::color(raw.listed),
        edge: theme::color(raw.edge),
        soft: theme::color(raw.soft),
        block: theme::color(raw.block),
    }
}

/// How a card's body is drawn where it sits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Body {
    /// In front, floating over the page.
    Front,
    /// Peeking out behind: plain paper a shade toward the ink, so the pile
    /// reads as layers rather than slivers of other colours.
    Behind,
    /// In the opened list, flat on the panel with no edge of its own.
    Listed,
}

/// The card's body alone, as a toast behind the front one shows: the
/// theme's paper in front, its tone's block of time drawn over it; in the
/// list, its tone drains into the paper as its time `left` runs out.
pub(super) fn shell(ui: &Ui, rect: Rect, (tone, left): (Tone, f32), body: Body, t: &Tokens) {
    let radius = CornerRadius::from(t.radius_lg);
    let painter = ui.painter();
    let (fill, edge) = paint_of(&look(tone, t), body, left, t);
    if body == Body::Front {
        painter.add(ui.style().visuals.popup_shadow.as_shape(rect, radius));
    }
    painter.rect(
        rect,
        radius,
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
}

/// A body's fill and edge, `left` of its time to go.
fn paint_of(swatch: &Swatch, body: Body, left: f32, t: &Tokens) -> (Color32, Color32) {
    let toned = |paper: Color32, tint: Color32| paper.lerp_to_gamma(tint, left.clamp(0.0, 1.0));
    match body {
        Body::Front => (t.surface, swatch.edge),
        Body::Behind => (t.surface.lerp_to_gamma(t.ink, 0.06), t.line),
        Body::Listed => {
            let paper = t.surface.lerp_to_gamma(t.ink, 0.04);
            (toned(paper, swatch.listed), Color32::TRANSPARENT)
        }
    }
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

/// The time left as a block of the tone, its edge feathered away.
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

/// The time left, the words and the buttons.
pub(super) fn content(ui: &mut Ui, rect: Rect, live: &Live, card: &Card, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let swatch = look(live.toast.tone, &t);
    let painter = ui.painter().clone();
    let left = live.remaining();
    if left > 0.0 && !live.listed {
        let inner = rect.shrink(1.0);
        let radius = t.radius_lg - 1.0;
        countdown(&painter, inner, radius, swatch.block, left);
    }
    // The words and buttons share the card's middle, however many lines.
    let middle = rect.center().y;
    let words = card.galley.size().y;
    let text = pos2(rect.left() + LEAD, (middle - words / 2.0).round());
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
    // A real button: the soft tone with a hairline of it, not a bare label.
    let button = Button::new(action_job(ui.ctx(), live, swatch.mark))
        .fill(swatch.soft)
        .stroke(Stroke::new(1.0, swatch.mark.gamma_multiply(0.35)))
        .corner_radius(7);
    let pressed = ui.put(at, button).on_hover_cursor(CursorIcon::PointingHand);
    pressed.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, live.toast.label)
    });
    if pressed.clicked() {
        out.push(Sent::Engine((**command).clone()));
        out.push(Sent::Ui(UiCommand::CloseToast(live.id)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_appearance::{Appearance, BaseMode};

    #[test]
    fn tones_use_distinct_theme_inflected_swatches() {
        let light = Tokens::new(&Appearance::default());
        let sepia = Tokens::new(&Appearance {
            tint_hue: 34,
            tint_strength: 25,
            ..Appearance::default()
        });
        let dark = Tokens::new(&Appearance {
            base: BaseMode::Dark,
            ..Appearance::default()
        });
        let info = look(Tone::Info, &light);
        let warn = look(Tone::Warning, &light);
        let err = look(Tone::Error, &light);
        assert_ne!(info.mark, light.accent);
        assert_ne!(info.listed, light.surface);
        assert_ne!(info.mark, warn.mark);
        assert_ne!(warn.mark, err.mark);
        assert_ne!(info.mark, err.mark);
        assert_ne!(info.listed, look(Tone::Info, &sepia).listed);
        let mid = light.toward(&dark, 0.5);
        assert_ne!(mid.toast, light.toast);
        assert_ne!(mid.toast, dark.toast);
    }

    /// How far apart two colours are, channel by channel.
    fn apart(a: Color32, b: Color32) -> i32 {
        let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).abs();
        d(a.r(), b.r()) + d(a.g(), b.g()) + d(a.b(), b.b())
    }

    #[test]
    fn a_card_s_time_is_a_bold_block_over_the_theme_s_paper() {
        let t = Tokens::new(&Appearance::default());
        let err = look(Tone::Error, &t);
        let fresh = paint_of(&err, Body::Front, 1.0, &t);
        let spent = paint_of(&err, Body::Front, 0.0, &t);
        assert_eq!(fresh, (t.surface, err.edge), "paper behind the block");
        assert_eq!(fresh, spent, "the paper itself never changes");
        let bold = err.block;
        let shade = apart(bold, t.surface);
        let old = theme::color(t.toast.error.fill);
        let bolder = shade > apart(old, t.surface) * 2;
        assert!(bolder, "bolder than the old tone: {shade}");
        assert!(apart(bold, t.ink) > 200, "the words still read on it");
        let dark = Tokens::new(&Appearance {
            base: BaseMode::Dark,
            ..Appearance::default()
        });
        let night = look(Tone::Warning, &dark).block;
        assert!(apart(night, dark.ink) > 200, "and on a dark theme");
    }

    #[test]
    fn a_listed_card_drains_from_its_tone_into_the_theme() {
        let t = Tokens::new(&Appearance::default());
        let err = look(Tone::Error, &t);
        let fresh = paint_of(&err, Body::Listed, 1.0, &t);
        let spent = paint_of(&err, Body::Listed, 0.0, &t);
        assert_eq!(fresh.0, err.listed, "arrives in its tone");
        assert_ne!(fresh, spent, "and drains");
    }
}
