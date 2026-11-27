//! Searchable model-provider selection.

use egui::{CornerRadius, Id, Ui};
use mareader_settings::{ModelPreset, Provider};

use crate::side_list::{self, Spec};
use crate::widgets;

/// Apple only where it runs; a switch forgets the old provider's model.
pub(super) fn show(ui: &mut Ui, p: &mut ModelPreset) {
    let before = p.provider;
    let items: Vec<_> = Provider::ALL
        .into_iter()
        .filter(|q| cfg!(target_os = "macos") || *q != Provider::Apple || *q == before)
        .map(|q| (q, q.label().to_owned()))
        .collect();
    let id = Id::new(("ai-provider", p.id.0));
    widgets::combo(id)
        .width(super::PICK_W)
        .selected_text(before.label())
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            let spec = Spec {
                items: &items,
                find: "Find a provider",
                none: "No providers match",
                width: super::PICK_W,
                corners: CornerRadius::same(6),
            };
            let picked =
                side_list::dropdown(ui, id.with("search"), spec, &|_| true, |ui, q, label| {
                    ui.selectable_label(before == *q, label)
                        .on_hover_text(q.detail())
                        .clicked()
                });
            if let Some(q) = picked {
                p.provider = q;
                ui.close();
            }
        });
    if p.provider != before {
        p.model.clear();
    }
}
