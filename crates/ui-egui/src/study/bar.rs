//! The small bar over a selection: Copy, Explain, Look up, Read aloud,
//! Note, Ask AI, pens. Ask AI opens the skills under the bar.

use std::sync::Arc;

use egui::{Context, CornerRadius, Id, Margin, Rect, Sense, Stroke, Ui, vec2};
use mareader_appearance::Rgb;
use mareader_library::Pen;
use mareader_settings::{BarItem, CustomSkill, HighlightSettings, PickColor, Skill};

use crate::bubble::{Bubble, Side};
use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::theme::{Tokens, color};
use crate::widgets;

/// A colour swatch's size in the picker row.
const SWATCH: f32 = 20.0;
/// The skills list's least width.
const SKILLS_W: f32 = 200.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPress {
    Copy,
    Explain,
    LookUp,
    Note,
    ReadAloud,
    /// A highlighter; a colour if the reader picked one.
    Highlight(Pen, Option<Rgb>),
    /// A skill from Ask AI's list.
    Skill(Skill),
}

/// What is open under the bar: a pen's colours, or the skills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    Pen(Pen),
    Skills,
}

/// The reader's skills, by id and name, as the bar lists them.
type Offered = Arc<[(Skill, String)]>;

fn offered_id() -> Id {
    Id::new("study-bar-skills")
}

/// Hands the bar the reader's skills, once a frame; the bar is drawn deep
/// inside each pane, away from the settings.
pub fn offer(ctx: &Context, custom: &[CustomSkill]) {
    let now: Vec<(Skill, String)> = (custom.iter())
        .map(|s| (Skill::Custom(s.id), s.title().to_owned()))
        .collect();
    let kept: Option<Offered> = ctx.data(|d| d.get_temp(offered_id()));
    if kept.as_deref() != Some(now.as_slice()) {
        ctx.data_mut(|d| d.insert_temp(offered_id(), Offered::from(now)));
    }
}

/// The icon a highlighter's button shows.
pub fn pen_icon(pen: Pen) -> Icon {
    match pen {
        Pen::Full => Icon::Highlight,
        Pen::Half => Icon::HalfHighlight,
        Pen::Underline => Icon::Underline,
    }
}

/// The bar over `selected` (under it with no room), pointing at it.
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
    // What is out under the bar: a pen's colours, or the skills.
    let open_id = id.with("pen");
    let mut open: Option<Open> = ctx.data(|d| d.get_temp(open_id)).flatten();
    if look.pick == PickColor::Default && matches!(open, Some(Open::Pen(_))) {
        open = None;
    }
    let custom: Offered = ctx.data(|d| d.get_temp(offered_id())).unwrap_or_default();
    let bubble = Bubble {
        id,
        at: selected,
        room,
        width: room.width(),
        margin: Margin::same(4),
        snug: true,
        prefer: Side::Above,
    };
    // Over its words, the list opens above the bar so the bar stays put.
    let above = Bubble::side_of(ctx, id) == Some(Side::Above);
    let (was, width_id, listed) = (open, id.with("bar-width"), open == Some(Open::Skills));
    let width = ctx
        .data(|d| d.get_temp(width_id))
        .unwrap_or(0.0_f32)
        .max(SKILLS_W);
    let (_, pressed) = bubble.show(ctx, &t, |ui, _| {
        // Hover moves glyphs before a selection closes this transient bar.
        let _act = anim::announce(ui, Act::Hover);
        let mut pressed = None;
        if listed && above {
            pressed = skills(ui, width, &custom, true).map(BarPress::Skill);
        }
        let bar = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let mut was_pen = None;
            for item in &look.bar {
                let pen = matches!(item, BarItem::Highlight(_));
                if was_pen.is_some_and(|was| was != pen) {
                    widgets::vrule(ui, 20.0);
                }
                was_pen = Some(pen);
                if let Some(press) = button(ui, *item, look, &mut open) {
                    pressed = Some(press);
                }
            }
        });
        ui.data_mut(|d| d.insert_temp(width_id, bar.response.rect.width()));
        if listed && !above {
            pressed = pressed.or(skills(ui, width, &custom, false).map(BarPress::Skill));
        }
        if let Some(Open::Pen(pen)) = open {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add_space(4.0);
                for c in look.colors(pen) {
                    if swatch(ui, c, &t).clicked() {
                        pressed = Some(BarPress::Highlight(pen, Some(c)));
                    }
                }
            });
            ui.add_space(2.0);
        }
        pressed
    });
    if open != was {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(open_id, open.filter(|_| pressed.is_none())));
    pressed
}

/// Ask AI's list: the built-ins in one row, then the reader's folded,
/// its rule on the bar's side.
fn skills(ui: &mut Ui, width: f32, custom: &[(Skill, String)], above: bool) -> Option<Skill> {
    let mut picked = None;
    ui.allocate_ui(vec2(width, 0.0), |ui| {
        ui.set_width(width);
        // Inside the list's width: the rule spans what it may take.
        if !above {
            ui.add_space(2.0);
            widgets::menu_rule(ui);
        }
        picked = crate::skill_menu::builtins(ui);
        if !custom.is_empty() {
            widgets::menu_rule(ui);
        }
        picked = picked.or(crate::skill_menu::custom(ui, custom));
        if above {
            widgets::menu_rule(ui);
            ui.add_space(2.0);
        }
    });
    picked
}

/// Ask AI, named in words: beside the AI Dictionary's mark it must not
/// read as a second one.
fn ask_ai(ui: &mut Ui, on: bool) -> egui::Response {
    const LABEL: &str = "Ask AI";
    let t = Tokens::get(ui.ctx());
    let resp = ui
        .push_id("ask-ai", |ui| {
            let _act = anim::assume(ui, Act::Click);
            let text = egui::RichText::new(LABEL).size(t.text_small);
            let glyph = anim::glyph(ui, Icon::Ai, t.icon, LABEL);
            let button = egui::Button::image_and_text(glyph, text)
                .image_tint_follows_text_color(true)
                .frame_when_inactive(false)
                .selected(on)
                .min_size(vec2(0.0, t.control));
            let resp = ui.add(button);
            let x = resp.rect.left() + ui.spacing().button_padding.x + t.icon * 0.5;
            let mark =
                Rect::from_center_size(egui::pos2(x, resp.rect.center().y), vec2(t.icon, t.icon));
            anim::play_at(ui, &resp, Icon::Ai, LABEL, mark, on);
            resp
        })
        .inner;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, LABEL));
    resp
}

/// An action as its icon alone; the name is its tooltip.
fn action(ui: &mut Ui, icon: Icon, label: &str) -> bool {
    widgets::icon_button(ui, icon, label, false).clicked()
}

/// One of the bar's buttons; a highlighter shows its colour underneath.
fn button(
    ui: &mut Ui,
    item: BarItem,
    look: &HighlightSettings,
    open: &mut Option<Open>,
) -> Option<BarPress> {
    match item {
        BarItem::Copy => action(ui, Icon::Copy, "Copy").then_some(BarPress::Copy),
        BarItem::Explain => action(ui, Icon::Ai, "AI Dictionary").then_some(BarPress::Explain),
        BarItem::LookUp => action(ui, Icon::LookUp, "Look up").then_some(BarPress::LookUp),
        BarItem::Note => action(ui, Icon::Note, "Note").then_some(BarPress::Note),
        BarItem::ReadAloud => {
            action(ui, Icon::ReadAloud, "Read aloud").then_some(BarPress::ReadAloud)
        }
        BarItem::Skills => {
            let on = *open == Some(Open::Skills);
            if ask_ai(ui, on).clicked() {
                *open = if on { None } else { Some(Open::Skills) };
            }
            None
        }
        BarItem::Highlight(pen) => {
            let on = *open == Some(Open::Pen(pen));
            let resp = widgets::icon_button(ui, pen_icon(pen), pen.label(), on);
            let strip =
                Rect::from_center_size(resp.rect.center_bottom() - vec2(0.0, 5.0), vec2(14.0, 2.5));
            let fill = color(look.color(pen));
            ui.painter().rect_filled(strip, CornerRadius::same(1), fill);
            if !resp.clicked() {
                return None;
            }
            if look.pick == PickColor::Default {
                return Some(BarPress::Highlight(pen, None));
            }
            *open = if on { None } else { Some(Open::Pen(pen)) };
            None
        }
    }
}

/// A round colour to pick, named by its hex for screen readers.
fn swatch(ui: &mut Ui, c: Rgb, t: &Tokens) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
    let ring = if resp.hovered() { t.ink } else { t.line };
    let painter = ui.painter();
    painter.circle_filled(rect.center(), SWATCH * 0.5 - 1.0, color(c));
    painter.circle_stroke(rect.center(), SWATCH * 0.5 - 1.0, Stroke::new(1.0, ring));
    let name = c.to_hex();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
