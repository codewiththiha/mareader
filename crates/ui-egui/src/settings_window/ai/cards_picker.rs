//! Study writer choice and per-model request sizes.

use crate::icons::Icon;
use crate::widgets;
use egui::Ui;
use mareader_settings::{ModelPreset, PresetId};

/// Study's writer picker keeps each model's request size beside its name.
pub(super) fn cards_picker(
    ui: &mut Ui,
    salt: &str,
    presets: &mut [ModelPreset],
    chosen: &mut Option<PresetId>,
    none: &str,
) {
    let shown = chosen
        .and_then(|id| presets.iter().find(|p| p.id == id))
        .map(ModelPreset::title)
        .unwrap_or_else(|| none.to_owned());
    widgets::combo(salt)
        .width(240.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .height(320.0)
        .selected_text(shown)
        .wrap_mode(egui::TextWrapMode::Truncate)
        .show_ui(ui, |ui| {
            ui.set_width(240.0);
            if ui.selectable_label(chosen.is_none(), none).clicked() {
                *chosen = None;
                ui.close();
            }
            for p in presets {
                let title = p.title();
                ui.horizontal(|ui| {
                    let name = egui::Button::selectable(*chosen == Some(p.id), &title).truncate();
                    if ui.add_sized([130.0, 26.0], name).clicked() {
                        *chosen = Some(p.id);
                        ui.close();
                    }
                    let max = mareader_study::mode::PER_REQUEST_MAX;
                    let less = format!("Less cards per request for {title}");
                    let more = format!("More cards per request for {title}");
                    if ui
                        .add_enabled_ui(p.cards_per_request > 1, |ui| {
                            widgets::small_icon_button(ui, Icon::Minus, &less)
                        })
                        .inner
                        .clicked()
                    {
                        p.cards_per_request = p.cards_per_request.saturating_sub(1).max(1);
                    }
                    ui.add_sized(
                        [20.0, 24.0],
                        egui::Label::new(p.cards_per_request.to_string()),
                    );
                    if ui
                        .add_enabled_ui(p.cards_per_request < max, |ui| {
                            widgets::small_icon_button(ui, Icon::Plus, &more)
                        })
                        .inner
                        .clicked()
                    {
                        p.cards_per_request = p.cards_per_request.saturating_add(1).min(max);
                    }
                });
            }
        });
}
