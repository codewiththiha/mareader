//! Searchable choices in Read aloud settings.

use egui::{CornerRadius, Id, Ui};
use mareader_settings::ReadAloudSettings;
use mareader_speech::{Build, Model};

use super::{ALL, idle_label, language_name};
use crate::side_list::{self, Spec};
use crate::widgets;

/// Which build a first download brings; keeps its explanatory tooltips.
pub(super) fn build_pick(ui: &mut Ui, model: &'static Model, on: &Build) -> Option<&'static str> {
    let mut pick = None;
    let items: Vec<_> = model
        .builds
        .iter()
        .map(|b| {
            let size = crate::dict::megabytes(model.bytes_with(b));
            (b.id, format!("{} · {size}", b.name))
        })
        .collect();
    let shown = widgets::combo(("first build", model.id))
        .selected_text(on.name)
        .width(72.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            let spec = Spec {
                items: &items,
                find: "Find a build",
                none: "No builds match",
                width: 200.0,
                corners: CornerRadius::same(6),
            };
            pick = side_list::dropdown(
                ui,
                Id::new(("first-build-search", model.id)),
                spec,
                &|_| true,
                |ui, id, name| {
                    ui.selectable_label(*id == on.id, name)
                        .on_hover_text(
                            model
                                .builds
                                .iter()
                                .find(|b| b.id == *id)
                                .map_or("", |b| b.about),
                        )
                        .clicked()
                },
            );
            if pick.is_some() {
                ui.close();
            }
        });
    let label = format!("{} build", model.name);
    shown
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, &label));
    shown.response.on_hover_text(on.about);
    pick.filter(|id| *id != on.id)
}

pub(super) fn idle(ui: &mut Ui, selected: &mut u32) {
    let items: Vec<_> = ReadAloudSettings::IDLE_MIN
        .into_iter()
        .map(|n| (n, idle_label(n)))
        .collect();
    crate::dropdown::select(
        ui,
        Id::new("model-idle"),
        idle_label(*selected),
        150.0,
        selected,
        &items,
    );
}

pub(super) fn language(ui: &mut Ui, engine: &str, languages: &[String], selected: &mut String) {
    let shown = match selected.as_str() {
        ALL => "All languages".to_owned(),
        tag => language_name(tag),
    };
    let mut items = vec![(ALL.to_owned(), "All languages".to_owned())];
    items.extend(
        languages
            .iter()
            .map(|tag| (tag.clone(), language_name(tag))),
    );
    crate::dropdown::select(
        ui,
        Id::new(("read-aloud-language", engine)),
        shown,
        180.0,
        selected,
        &items,
    );
}
