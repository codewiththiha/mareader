//! Compact previews of selection answers in Notes.

use egui::{FontId, Id, Rect, Sense, TextFormat, Ui, pos2, vec2};
use mareader_engine::command::Command;
use mareader_engine::library::SaveStatus;
use mareader_engine::skills::Action;
use mareader_engine::toast::Toast;

use super::{Reading, SelectionRow};
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};

const PREVIEW_CHARS: usize = 180;

fn preview(source: &str) -> Option<String> {
    let mut chars = source.chars();
    let first: String = chars.by_ref().take(PREVIEW_CHARS).collect();
    chars.next().map(|_| format!("{}…", first.trim_end()))
}

pub(super) fn entry(ui: &mut Ui, answer: &SelectionRow, reading: &Reading, out: &mut Commands) {
    let state = match (answer.pinned, answer.pin_status) {
        (false, _) => "This session",
        (true, SaveStatus::Pending) => "Saving…",
        (true, SaveStatus::Failed) => "Save failed",
        (true, SaveStatus::Saved) => "Pinned",
    };
    let id = Id::new(("notes-selection-expanded", answer.id));
    let expanded = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    let short = preview(&answer.source);
    let source = match (&short, expanded) {
        (Some(short), false) => short.as_str(),
        _ => answer.source.as_str(),
    };
    let label = format!("{} · {state}\n{source}", answer.name);
    let more = short
        .as_ref()
        .map(|_| if expanded { "Show less" } else { "Show more" });
    let (row, link) = draw(ui, id, &label, more, answer.found);
    if link {
        ui.data_mut(|d| d.insert_temp(id, !expanded));
    } else if row && answer.found {
        out.push(Sent::Engine(Command::Skill(Action::Show(answer.id))));
        let back = Command::GoTo {
            pane: reading.pane,
            page: reading.current,
        };
        let message = format!(
            "Opened selection · from page {}",
            reading.global_current() + 1
        );
        out.push(Sent::Ui(UiCommand::Notify(Toast::info(message).undo(back))));
    }
    if !answer.found {
        ui.label("This passage could not be found in the current layout.");
    }
}

/// Draws the row and its independently clickable inline expansion link.
fn draw(ui: &mut Ui, id: Id, label: &str, more: Option<&str>, enabled: bool) -> (bool, bool) {
    let t = Tokens::get(ui.ctx());
    let font = FontId::proportional(t.text);
    let mut job = egui::text::LayoutJob::default();
    job.append(label, 0.0, TextFormat::simple(font.clone(), t.ink));
    let link_text = more.map(|text| text.replace(' ', "\u{a0}"));
    if let Some(text) = &link_text {
        job.append("  ", 0.0, TextFormat::simple(font.clone(), t.ink));
        job.append(text, 0.0, TextFormat::simple(font.clone(), t.accent));
    }
    job.wrap.max_width = (ui.available_width() - 20.0).max(20.0);
    let end = job.text.chars().count();
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    let size = vec2(ui.available_width(), galley.size().y + 16.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect.shrink2(vec2(6.0, 2.0)), t.radius, t.hover);
    }
    let origin = rect.min + vec2(10.0, 8.0);
    ui.painter().galley(origin, galley.clone(), t.ink);
    let mut clicked = false;
    if let (Some(text), Some(name)) = (link_text, more) {
        let link_width = ui
            .ctx()
            .fonts_mut(|f| f.layout_no_wrap(text, font, t.accent))
            .size()
            .x;
        let end = galley
            .pos_from_cursor(egui::text::CCursor::new(end))
            .translate(origin.to_vec2());
        let at = Rect::from_min_max(pos2(end.left() - link_width, end.top()), end.right_bottom());
        let link = ui.interact(at, id.with("more"), Sense::click());
        link.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
        clicked = link
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked();
    }
    (response.clicked(), clicked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_limits_unicode_without_losing_the_source() {
        assert!(preview(&"é".repeat(PREVIEW_CHARS)).is_none());
        let source = "é".repeat(PREVIEW_CHARS + 2);
        let short = preview(&source).unwrap();
        assert_eq!(short.chars().count(), PREVIEW_CHARS + 1);
        assert!(short.ends_with('…'));
        assert_eq!(source.chars().count(), PREVIEW_CHARS + 2);
    }
}
