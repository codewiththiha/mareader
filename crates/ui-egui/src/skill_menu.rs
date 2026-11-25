//! Skills in a menu: the built-ins side by side in one row, and the
//! reader's own folded into one more, a [`side_list`] row: hover opens
//! the list beside it, and typing finds a skill.
//!
//! [`side_list`]: crate::side_list

use egui::{
    CornerRadius, FontId, Rect, Response, Sense, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_settings::{PageTask, Skill};

use crate::icons::Icon;
use crate::side_list::{self, Spec};
use crate::theme::Tokens;
use crate::widgets;

/// The folded row's name.
pub const ROW: &str = "Custom skills";
/// The search field's name.
pub const FIND: &str = "Find a skill";

/// The list beside the row: narrower than the menu it opens from.
const WIDTH: f32 = 196.0;
const CELL_H: f32 = 34.0;

/// Translate, Summarize and Simplify side by side; the one pressed.
pub fn builtins(ui: &mut Ui) -> Option<Skill> {
    let t = Tokens::get(ui.ctx());
    let gap = 2.0;
    let cell = vec2((ui.available_width() - gap * 2.0) / 3.0, CELL_H);
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for task in [PageTask::Translate, PageTask::Summarize, PageTask::Simplify] {
            if cell_button(ui, task, cell, &t).clicked() {
                picked = Some(Skill::Builtin(task));
            }
        }
    });
    picked
}

/// The icon a built-in shows.
pub fn icon(task: PageTask) -> Icon {
    match task {
        PageTask::Translate => Icon::Translate,
        PageTask::Summarize => Icon::Summarize,
        PageTask::Simplify => Icon::Simplify,
    }
}

/// One built-in: its icon and name, centred in its share of the row.
fn cell_button(ui: &mut Ui, task: PageTask, size: Vec2, t: &Tokens) -> Response {
    let label = task.label();
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let fill = match (
        resp.is_pointer_button_down_on(),
        resp.hovered() || resp.has_focus(),
    ) {
        (true, _) => Some(t.pressed),
        (false, true) => Some(t.hover),
        _ => None,
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let galley = (ui.painter()).layout_no_wrap(label.to_owned(), FontId::proportional(13.5), t.ink);
    let x = rect.center().x - (15.0 + 6.0 + galley.size().x) / 2.0;
    let y = rect.center().y;
    let tint = if fill.is_some() { t.ink } else { t.muted };
    let mark = Rect::from_min_size(pos2(x, y - 7.5), Vec2::splat(15.0));
    icon(task).paint(ui, mark, tint);
    let at = pos2(x + 21.0, y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, t.ink);
    resp
}

/// The reader's skills folded into one row; none, no row. The one picked.
pub fn custom(ui: &mut Ui, skills: &[(Skill, String)]) -> Option<Skill> {
    if skills.is_empty() {
        return None;
    }
    let count = skills.len().to_string();
    let row = |ui: &mut Ui| widgets::menu_row_chevron(ui, Icon::Ai, ROW, Some(&count));
    let cell = |ui: &mut Ui, _: &Skill, name: &str| {
        widgets::menu_row(ui, Icon::Ai, name, None, false).clicked()
    };
    let spec = Spec {
        items: skills,
        find: FIND,
        none: "No skill matches",
        width: WIDTH,
        corners: CornerRadius::same(8),
    };
    side_list::show(ui, spec, row, cell).picked
}
