//! Text-only skill rows in the selection menu.

use egui::{Align2, CornerRadius, FontId, Rect, Response, Sense, Ui, Vec2, pos2, vec2};
use mareader_settings::{PageTask, Skill};

use crate::icons::Icon;
use crate::side_list::{self, Spec};
use crate::theme::Tokens;

const HEIGHT: f32 = 25.0;
const WIDTH: f32 = 196.0;

pub(super) fn builtins(ui: &mut Ui, parent: Option<Rect>) -> Option<super::BarPress> {
    let choices = crate::language_menu::offered(ui.ctx());
    let shown = crate::language_menu::show_beside_action(ui, &choices, parent, |ui| {
        row(ui, "Translate", true)
    });
    let mut chosen = shown.picked.map(super::BarPress::Translate);
    if chosen.is_none() && shown.row.clicked() {
        chosen = choices.current.map(super::BarPress::TranslateCurrent);
    }
    for task in [PageTask::Summarize, PageTask::Simplify] {
        if row(ui, task.label(), false).clicked() {
            chosen = Some(super::BarPress::Skill(Skill::Builtin(task)));
        }
    }
    chosen
}

pub(super) fn custom(
    ui: &mut Ui,
    skills: &[(Skill, String)],
    parent: Option<Rect>,
) -> Option<Skill> {
    let spec = Spec {
        items: skills,
        find: crate::skill_menu::FIND,
        none: if skills.is_empty() {
            "No custom skills yet"
        } else {
            "No skill matches"
        },
        width: WIDTH,
        corners: CornerRadius::same(8),
    };
    side_list::show_beside(
        ui,
        spec,
        parent,
        |ui| row(ui, crate::skill_menu::ROW, true),
        |ui, _, name| row(ui, name, false).clicked(),
    )
    .picked
}

fn row(ui: &mut Ui, label: &str, arrow: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if response.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 6.0, t.pressed);
    } else if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 6.0, t.hover);
    }
    let end = if arrow { 24.0 } else { 8.0 };
    let clip = Rect::from_min_max(rect.min, pos2(rect.right() - end, rect.bottom()));
    ui.painter().with_clip_rect(clip).text(
        pos2(rect.left() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.5),
        t.ink,
    );
    if arrow {
        let at = Rect::from_center_size(
            pos2(rect.right() - 14.0, rect.center().y),
            Vec2::splat(14.0),
        );
        Icon::ChevronRight.paint(ui, at, t.muted);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
