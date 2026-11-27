//! Continuous defaults live inside the page-action submenus.

use egui::{CornerRadius, Rect, Response, Ui};
use mareader_settings::PageTask;

use crate::icons::Icon;
use crate::language_menu::{self, Choices};
use crate::side_list::{self, Shown, Spec};
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;

fn toggle(ui: &mut Ui, task: PageTask, on: &mut bool, out: &mut Commands) -> Response {
    let label = match task {
        PageTask::Translate => "Translate continuously",
        PageTask::Simplify => "Simplify continuously",
        PageTask::Summarize => "Summarize continuously",
    };
    let response = widgets::menu_switch(ui, Icon::Repeat, "Continuous", on);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label)
    });
    if response.changed() {
        out.push(Sent::Ui(UiCommand::PageContinuous(task, *on)));
    }
    response
}

pub(super) fn translate(
    ui: &mut Ui,
    choices: &Choices,
    parent: Option<Rect>,
    (label, current): (&str, Option<&str>),
    group: bool,
    on: &mut bool,
    out: &mut Commands,
) -> Shown<String> {
    let mut header = |ui: &mut Ui| {
        let response = toggle(ui, PageTask::Translate, on, out);
        widgets::menu_rule(ui);
        response
    };
    language_menu::show_beside_with(
        ui,
        choices,
        parent,
        |ui| widgets::menu_row_chevron(ui, Icon::Translate, label, current),
        if group { None } else { Some(&mut header) },
    )
}

pub(super) fn simplify(
    ui: &mut Ui,
    parent: Option<Rect>,
    label: &str,
    group: bool,
    on: &mut bool,
    out: &mut Commands,
) -> Response {
    if group {
        return widgets::menu_row(ui, Icon::Simplify, label, None, false);
    }
    let spec = Spec::<()> {
        items: &[],
        find: "",
        none: "",
        width: 196.0,
        corners: CornerRadius::same(8),
    };
    let mut header = |ui: &mut Ui| toggle(ui, PageTask::Simplify, on, out);
    side_list::show_beside_with(
        ui,
        spec,
        parent,
        |ui| {
            widgets::menu_row_chevron(ui, Icon::Simplify, label, None)
                .on_hover_text("Rewrites the text in plain English")
        },
        Some(&mut header),
        |_, _, _| false,
    )
    .row
}
