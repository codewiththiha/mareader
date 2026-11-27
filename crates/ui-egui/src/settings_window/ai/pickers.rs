//! Searchable model and language choices shared by settings tasks.

use egui::{CornerRadius, Id, Ui};
use mareader_dict::LANGUAGES;
use mareader_settings::{ModelPreset, PresetId};

use crate::side_list::{self, Spec};
use crate::widgets;

const WIDTH: f32 = 240.0;

/// A saved model for a task, or `none`.
pub(crate) fn preset_picker(
    ui: &mut Ui,
    salt: &str,
    presets: &[ModelPreset],
    chosen: &mut Option<PresetId>,
    none: &str,
) {
    let title = |id: PresetId| presets.iter().find(|p| p.id == id).map(ModelPreset::title);
    let shown = chosen.and_then(title).unwrap_or_else(|| none.to_owned());
    let mut items = vec![(None, none.to_owned())];
    items.extend(presets.iter().map(|p| (Some(p.id), p.title())));
    widgets::combo(salt)
        .width(WIDTH)
        .wrap_mode(egui::TextWrapMode::Truncate)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            let spec = Spec {
                items: &items,
                find: "Find a model",
                none: "No model matches",
                width: WIDTH,
                corners: CornerRadius::same(6),
            };
            let picked = side_list::dropdown(
                ui,
                Id::new((salt, "search")),
                spec,
                &|_| true,
                |ui, id, name| {
                    ui.add_sized(
                        [WIDTH, 26.0],
                        egui::Button::selectable(*chosen == *id, name).truncate(),
                    )
                    .clicked()
                },
            );
            if let Some(id) = picked {
                *chosen = id;
                ui.close();
            }
        });
}

/// The default language or a listed one; ones Apple can't write are greyed.
pub(crate) fn language_picker(
    ui: &mut Ui,
    (salt, none): (&str, &str),
    chosen: &mut Option<String>,
    speaks: &dyn Fn(&str) -> Option<bool>,
) {
    let name = chosen.as_deref().and_then(mareader_settings::language_name);
    let mut items = vec![(None, none.to_owned())];
    items.extend(
        LANGUAGES
            .iter()
            .map(|l| (Some(l.tag.to_owned()), l.name.to_owned())),
    );
    let enabled = |tag: &Option<String>| tag.as_deref().is_none_or(|t| speaks(t) != Some(false));
    widgets::combo(salt)
        .width(WIDTH)
        .height(320.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .selected_text(name.unwrap_or(none))
        .show_ui(ui, |ui| {
            let spec = Spec {
                items: &items,
                find: "Find a language",
                none: "No language matches",
                width: WIDTH,
                corners: CornerRadius::same(6),
            };
            let picked = side_list::dropdown(
                ui,
                Id::new((salt, "search")),
                spec,
                &enabled,
                |ui, tag, name| {
                    let button = egui::Button::selectable(*chosen == *tag, name).truncate();
                    let response = ui.add_enabled(enabled(tag), button);
                    let clicked = response.clicked();
                    response
                        .on_disabled_hover_text("Apple Intelligence doesn't write this language");
                    clicked
                },
            );
            if let Some(tag) = picked {
                *chosen = tag;
                ui.close();
            }
        });
}
