//! The page menu's own rows: reading aloud from a word, and the reader's
//! skills, each run on the page with its answer in a card.

use egui::text::{LayoutJob, TextFormat};
use egui::{CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use mareader_engine::command::Command;
use mareader_engine::skills::Action as SkillAction;
use mareader_settings::{AiSettings, Skill};

use super::super::Menu;
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

/// The reader's skills, under their heading; none, no heading. True once
/// one is picked: it runs on the page, its card pointing where the menu
/// opened.
pub(super) fn custom(ui: &mut Ui, m: &Menu, ai: &AiSettings, out: &mut Commands) -> bool {
    let skills = &ai.tasks.skills.custom;
    if skills.is_empty() {
        return false;
    }
    widgets::menu_rule(ui);
    widgets::menu_section(ui, None, "Custom skills");
    let point = m.pin.as_ref().map_or(pos2(0.0, 0.0), |p| p.point);
    for skill in skills {
        if widgets::menu_row(ui, Icon::Ai, skill.title(), None, false).clicked() {
            out.push(Sent::Engine(Command::Skill(SkillAction::RunPage {
                pane: m.pane,
                page: m.page,
                x: point.x,
                y: point.y,
                skill: Skill::Custom(skill.id),
            })));
            return true;
        }
    }
    false
}

/// Read aloud's row on the `30pt` menu grid, icon in the lit word's tone.
pub(super) fn aloud_row(ui: &mut Ui, word: Option<&str>, t: &Tokens) -> egui::Response {
    let warm = crate::read_aloud::marker::warm(t);
    let font = FontId::proportional(13.5);
    let (label, job) = match word {
        Some(w) => {
            let w = short(w);
            let mut job = LayoutJob::default();
            job.append(
                "Read aloud from ",
                0.0,
                TextFormat::simple(font.clone(), t.ink),
            );
            job.append(
                &format!("“{w}”"),
                0.0,
                TextFormat {
                    font_id: font,
                    color: t.ink,
                    underline: Stroke::new(1.5, warm),
                    ..Default::default()
                },
            );
            (format!("Read aloud from “{w}”"), job)
        }
        None => {
            let label = "Read aloud from page start".to_owned();
            let mut job = LayoutJob::default();
            job.append(&label, 0.0, TextFormat::simple(font, t.ink));
            (label, job)
        }
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if resp.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), t.pressed);
    } else if resp.hovered() || resp.has_focus() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), t.hover);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let y = rect.center().y;
    let icon_rect = Rect::from_min_size(pos2(rect.left() + 8.0, y - 7.5), Vec2::splat(15.0));
    Icon::ReadAloud.paint(ui, icon_rect, warm);
    let text_x = rect.left() + 32.0;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let clip = Rect::from_min_max(
        pos2(text_x, rect.top()),
        pos2(rect.right() - 8.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(clip).galley(
        pos2(text_x, y - galley.size().y * 0.5),
        galley,
        t.ink,
    );
    resp
}

/// A long word cut short, so the row keeps to one line.
fn short(word: &str) -> String {
    const MOST: usize = 16;
    match word.chars().count() > MOST {
        true => format!("{}…", word.chars().take(MOST - 1).collect::<String>()),
        false => word.to_owned(),
    }
}
