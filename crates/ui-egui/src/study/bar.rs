//! The actions, highlighters and skills over selected text.

mod skills;

use std::sync::Arc;

use egui::{Context, CornerRadius, Id, Margin, Rect, ScrollArea, Sense, Stroke, Ui, vec2};
use mareader_appearance::Rgb;
use mareader_library::Pen;
use mareader_settings::{BarItem, CustomSkill, HighlightSettings, PickColor, Skill};

use crate::bubble::{Bubble, Side};
use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::theme::{Tokens, color};
use crate::widgets;

const WIDTH: f32 = 248.0;
const SWATCH: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPress {
    Copy,
    Explain,
    LookUp,
    Note,
    ReadAloud,
    ManageSkills,
    Highlight(Pen, Option<Rgb>),
    Skill(Skill),
}

type Offered = Arc<[(Skill, String)]>;

fn offered_id() -> Id {
    Id::new("study-bar-skills")
}

pub fn offer(ctx: &Context, custom: &[CustomSkill]) {
    let now: Vec<(Skill, String)> = custom
        .iter()
        .map(|s| (Skill::Custom(s.id), s.title().to_owned()))
        .collect();
    let kept: Option<Offered> = ctx.data(|d| d.get_temp(offered_id()));
    if kept.as_deref() != Some(now.as_slice()) {
        ctx.data_mut(|d| d.insert_temp(offered_id(), Offered::from(now)));
    }
}

pub fn pen_icon(pen: Pen) -> Icon {
    match pen {
        Pen::Full => Icon::Highlight,
        Pen::Half => Icon::HalfHighlight,
        Pen::Underline => Icon::Underline,
    }
}

pub fn show(
    ctx: &Context,
    id: Id,
    selected: Rect,
    room: Rect,
    look: &HighlightSettings,
) -> Option<BarPress> {
    if look.bar.is_empty() {
        return None;
    }
    let t = Tokens::get(ctx);
    let open_id = id.with("pen");
    let mut pen: Option<Pen> = ctx.data(|d| d.get_temp(open_id)).flatten();
    if look.pick == PickColor::Default {
        pen = None;
    }
    let was = pen;
    let custom: Offered = ctx.data(|d| d.get_temp(offered_id())).unwrap_or_default();
    let bubble = Bubble {
        id,
        at: selected,
        room,
        width: WIDTH,
        margin: Margin::same(8),
        snug: false,
        prefer: Side::Above,
    };
    let (_, pressed) = bubble.show(ctx, &t, |ui, cap| {
        let _act = anim::announce(ui, Act::Hover);
        ScrollArea::vertical()
            .max_height(cap)
            .auto_shrink([false, true])
            .show(ui, |ui| contents(ui, look, &custom, &mut pen, &t))
            .inner
    });
    if pen != was {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(open_id, pen.filter(|_| pressed.is_none())));
    pressed
}

fn contents(
    ui: &mut Ui,
    look: &HighlightSettings,
    custom: &[(Skill, String)],
    pen: &mut Option<Pen>,
    t: &Tokens,
) -> Option<BarPress> {
    let mut pressed = None;
    let has = |item| look.bar.contains(&item);
    ui.spacing_mut().item_spacing.y = 2.0;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for item in &look.bar {
            let (icon, label, action) = match item {
                BarItem::Copy => (Icon::Copy, "Copy", BarPress::Copy),
                BarItem::ReadAloud => (Icon::ReadAloud, "Read aloud", BarPress::ReadAloud),
                BarItem::Highlight(p) => {
                    if let Some(action) = highlighter(ui, *p, look, pen) {
                        pressed = Some(action);
                    }
                    continue;
                }
                _ => continue,
            };
            if widgets::icon_button(ui, icon, label, false).clicked() {
                pressed = Some(action);
            }
        }
    });
    if let Some(p) = *pen {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for c in look.colors(p) {
                if swatch(ui, c, t).clicked() {
                    pressed = Some(BarPress::Highlight(p, Some(c)));
                }
            }
        });
    }
    for item in &look.bar {
        let (icon, label, action) = match item {
            BarItem::Explain => (Icon::Ai, "AI Dictionary", BarPress::Explain),
            BarItem::LookUp => (Icon::LookUp, "Look up", BarPress::LookUp),
            BarItem::Note => (Icon::Note, "Note", BarPress::Note),
            _ => continue,
        };
        if widgets::menu_row(ui, icon, label, None, false).clicked() {
            pressed = Some(action);
        }
    }
    if has(BarItem::Skills) {
        widgets::menu_rule(ui);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Skills")
                    .size(t.text_small)
                    .color(t.muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::small_icon_button(ui, Icon::Settings, "Manage skills").clicked() {
                    pressed = Some(BarPress::ManageSkills);
                }
            });
        });
        pressed = pressed.or(skills::builtins(ui).map(BarPress::Skill));
        pressed = pressed.or(skills::custom(ui, custom).map(BarPress::Skill));
    }
    pressed
}

fn highlighter(
    ui: &mut Ui,
    pen: Pen,
    look: &HighlightSettings,
    open: &mut Option<Pen>,
) -> Option<BarPress> {
    let on = *open == Some(pen);
    let response = widgets::icon_button(ui, pen_icon(pen), pen.label(), on);
    let strip = Rect::from_center_size(
        response.rect.center_bottom() - vec2(0.0, 5.0),
        vec2(14.0, 2.5),
    );
    ui.painter()
        .rect_filled(strip, CornerRadius::same(1), color(look.color(pen)));
    if !response.clicked() {
        return None;
    }
    if look.pick == PickColor::Default {
        return Some(BarPress::Highlight(pen, None));
    }
    *open = if on { None } else { Some(pen) };
    None
}

fn swatch(ui: &mut Ui, c: Rgb, t: &Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
    let ring = if resp.hovered() { t.ink } else { t.line };
    ui.painter()
        .circle_filled(rect.center(), SWATCH * 0.5 - 1.0, color(c));
    ui.painter()
        .circle_stroke(rect.center(), SWATCH * 0.5 - 1.0, Stroke::new(1.0, ring));
    let name = c.to_hex();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
