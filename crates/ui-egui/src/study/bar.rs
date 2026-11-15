//! The small bar over a selection: Copy, Explain, Look up, Read aloud,
//! Note, pens.

use egui::{Context, CornerRadius, Id, Margin, Rect, Sense, Stroke, Ui, vec2};
use mareader_appearance::Rgb;
use mareader_library::Pen;
use mareader_settings::{BarItem, HighlightSettings, PickColor};

use crate::bubble::{Bubble, Side};
use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::theme::{Tokens, color};
use crate::widgets;

/// A colour swatch's size in the picker row.
const SWATCH: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPress {
    Copy,
    Explain,
    LookUp,
    Note,
    ReadAloud,
    /// A highlighter; a colour if the reader picked one.
    Highlight(Pen, Option<Rgb>),
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
    // Which pen's colours are out, when the reader picks each time.
    let open_id = id.with("pen");
    let mut open: Option<Pen> = ctx.data(|d| d.get_temp(open_id)).flatten();
    if look.pick == PickColor::Default {
        open = None;
    }
    let bubble = Bubble {
        id,
        at: selected,
        room,
        width: room.width(),
        margin: Margin::same(4),
        snug: true,
        prefer: Side::Above,
    };
    let (_, pressed) = bubble.show(ctx, &t, |ui, _| {
        // Hover moves glyphs before a selection closes this transient bar.
        let _act = anim::announce(ui, Act::Hover);
        let mut pressed = None;
        ui.horizontal(|ui| {
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
        if let Some(pen) = open {
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
    ctx.data_mut(|d| d.insert_temp(open_id, open.filter(|_| pressed.is_none())));
    pressed
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
    open: &mut Option<Pen>,
) -> Option<BarPress> {
    match item {
        BarItem::Copy => action(ui, Icon::Copy, "Copy").then_some(BarPress::Copy),
        BarItem::Explain => action(ui, Icon::Ai, "AI Dictionary").then_some(BarPress::Explain),
        BarItem::LookUp => action(ui, Icon::LookUp, "Look up").then_some(BarPress::LookUp),
        BarItem::Note => action(ui, Icon::Note, "Note").then_some(BarPress::Note),
        BarItem::ReadAloud => {
            action(ui, Icon::ReadAloud, "Read aloud").then_some(BarPress::ReadAloud)
        }
        BarItem::Highlight(pen) => {
            let resp = widgets::icon_button(ui, pen_icon(pen), pen.label(), *open == Some(pen));
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
            *open = if *open == Some(pen) { None } else { Some(pen) };
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
