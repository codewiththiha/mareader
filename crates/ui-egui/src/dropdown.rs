//! Searchable fixed-width dropdowns built on the shared side-list behavior.

use egui::{CornerRadius, Id, Response, Ui};

use crate::side_list::{self, Spec};
use crate::widgets;

/// A single-choice dropdown; typing filters, arrows move, and Enter picks.
pub fn select<T: Clone + PartialEq>(
    ui: &mut Ui,
    id: Id,
    shown: impl Into<egui::WidgetText>,
    width: f32,
    chosen: &mut T,
    items: &[(T, String)],
) -> Response {
    widgets::combo(id)
        .selected_text(shown)
        .width(width)
        .wrap_mode(egui::TextWrapMode::Truncate)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            let spec = Spec {
                items,
                find: "Find an option",
                none: "No options match",
                width,
                corners: CornerRadius::same(6),
            };
            let picked =
                side_list::dropdown(ui, id.with("search"), spec, &|_| true, |ui, value, name| {
                    let button = egui::Button::selectable(*chosen == *value, name).truncate();
                    ui.add_sized([width, 26.0], button).clicked()
                });
            if let Some(value) = picked {
                *chosen = value;
                ui.close();
            }
        })
        .response
}
