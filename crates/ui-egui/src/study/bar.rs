//! Compact selection menu: skills above, common actions nearest the words.
mod skills;

use crate::bubble::{Bubble, Side};
use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::theme::{Tokens, color};
use crate::widgets;
use egui::{Context, CornerRadius, Id, Margin, Rect, ScrollArea, Sense, Stroke, Ui, vec2};
use mareader_appearance::Rgb;
use mareader_library::Pen;
use mareader_settings::{CustomSkill, HighlightSettings, PickColor, Skill};
use std::sync::Arc;

const SWATCH: f32 = 18.0;
const WIDTH: f32 = 180.0;
const HEIGHT: f32 = 216.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarPress {
    Copy,
    Explain,
    LookUp,
    Note,
    ReadAloud,
    ManageSkills,
    Highlight(Pen, Option<Rgb>),
    Skill(Skill),
    Translate(String),
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
    restricted: bool,
) -> Option<BarPress> {
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
        margin: Margin::same(6),
        snug: false,
        prefer: Side::Above,
    };
    let (_, pressed) = bubble.show(ctx, &t, |ui, cap| {
        let _act = anim::announce(ui, Act::Hover);
        let height = HEIGHT.min(cap).max(0.0);
        let height = if restricted {
            height.min(108.0)
        } else {
            height
        };
        ui.set_height(height);
        if height < 84.0 {
            ScrollArea::vertical()
                .id_salt("selection-tiny")
                .max_height(height)
                .show(ui, |ui| {
                    contents(ui, look, &custom, &mut pen, &t, 84.0, restricted)
                })
                .inner
        } else {
            contents(ui, look, &custom, &mut pen, &t, height, restricted)
        }
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
    height: f32,
    restricted: bool,
) -> Option<BarPress> {
    let mut pressed = None;
    ui.spacing_mut().item_spacing.y = 1.0;
    let palette = if pen.is_some() { 25.0 } else { 0.0 };
    let bottom = 37.0 + palette;
    let actions = 46.0;
    let body = (height - bottom - actions).max(0.0);
    ScrollArea::vertical()
        .id_salt("selection-skills")
        .max_height(body)
        .min_scrolled_height(body)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if !restricted {
                if skills_heading(ui, t) {
                    pressed = Some(BarPress::ManageSkills);
                }
                let builtin = skills::builtins(ui);
                let own = skills::custom(ui, custom).map(BarPress::Skill);
                pressed = pressed.take().or(builtin).or(own);
            }
        });
    widgets::menu_rule(ui);
    let action = action_row(ui, restricted);
    pressed = pressed.or(action);
    if let Some(p) = *pen {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            for c in look.colors(p) {
                if swatch(ui, c, t).clicked() {
                    pressed = Some(BarPress::Highlight(p, Some(c)));
                }
            }
        });
    }
    let bottom = bottom_actions(ui, look, pen);
    pressed.or(bottom)
}

fn action_row(ui: &mut Ui, restricted: bool) -> Option<BarPress> {
    let available = ui.available_width();
    let icon_width = (available / 5.0).max(32.0);
    let note_width = if restricted {
        available
    } else {
        (available - 2.0 * icon_width).max(80.0)
    };
    let mut pressed = None;
    let draw = |ui: &mut Ui, pressed: &mut Option<BarPress>| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                vec2(note_width, 36.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if widgets::menu_row(ui, Icon::Note, "Note", None, false).clicked() {
                        *pressed = Some(BarPress::Note);
                    }
                },
            );
            if !restricted {
                for (icon, label, action) in [
                    (Icon::Ai, "AI Dictionary", BarPress::Explain),
                    (Icon::LookUp, "Look up", BarPress::LookUp),
                ] {
                    ui.allocate_ui_with_layout(
                        vec2(icon_width, 36.0),
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            if compact_icon(ui, icon, label, false).clicked() {
                                *pressed = Some(action);
                            }
                        },
                    );
                }
            }
        });
    };
    if !restricted && note_width + 2.0 * icon_width > available {
        ScrollArea::horizontal()
            .id_salt("selection-other-actions")
            .max_width(available)
            .show(ui, |ui| draw(ui, &mut pressed));
    } else {
        draw(ui, &mut pressed);
    }
    pressed
}

fn skills_heading(ui: &mut Ui, t: &Tokens) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Skills")
                .size(t.text_small)
                .color(t.muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let manage = egui::Button::new(
                egui::RichText::new("Manage skills")
                    .size(t.text_small)
                    .color(t.muted),
            )
            .frame(false);
            clicked = ui
                .add(manage)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked();
        });
    });
    clicked
}

fn bottom_actions(
    ui: &mut Ui,
    look: &HighlightSettings,
    pen: &mut Option<Pen>,
) -> Option<BarPress> {
    let mut pressed = None;
    distributed(ui, 5, |ui, i| match i {
        0 => {
            if compact_icon(ui, Icon::Copy, "Copy", false).clicked() {
                pressed = Some(BarPress::Copy);
            }
        }
        1 => {
            if compact_icon(ui, Icon::ReadAloud, "Read aloud", false).clicked() {
                pressed = Some(BarPress::ReadAloud);
            }
        }
        _ => {
            let p = [Pen::Full, Pen::Half, Pen::Underline][i - 2];
            if let Some(action) = highlighter(ui, p, look, pen) {
                pressed = Some(action);
            }
        }
    });
    pressed
}

fn distributed(ui: &mut Ui, count: usize, mut draw: impl FnMut(&mut Ui, usize)) {
    let available = ui.available_width();
    let width = (available / count as f32).max(24.0);
    let row = |ui: &mut Ui, draw: &mut dyn FnMut(&mut Ui, usize)| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for i in 0..count {
                ui.allocate_ui_with_layout(
                    vec2(width, 36.0),
                    egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                    |ui| draw(ui, i),
                );
            }
        });
    };
    if width * count as f32 > available {
        ScrollArea::horizontal()
            .id_salt(("selection-row", count))
            .max_width(available)
            .show(ui, |ui| row(ui, &mut draw));
    } else {
        row(ui, &mut draw);
    }
}

fn compact_icon(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> egui::Response {
    if ui.available_width() < 36.0 {
        widgets::small_icon_button(ui, icon, label)
    } else {
        widgets::icon_button(ui, icon, label, selected)
    }
}

fn highlighter(
    ui: &mut Ui,
    pen: Pen,
    look: &HighlightSettings,
    open: &mut Option<Pen>,
) -> Option<BarPress> {
    let on = *open == Some(pen);
    let response = compact_icon(ui, pen_icon(pen), pen.label(), on);
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
