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

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use mareader_settings::AiSettings;

    #[test]
    fn typing_and_enter_picks_a_provider_and_clears_the_old_model() {
        let mut ai = AiSettings::default();
        let id = ai.add_preset(mareader_settings::Provider::Ollama, "old-model");
        let preset = ai.preset(id).unwrap().clone();
        let mut h = Harness::builder()
            .with_size(egui::vec2(600.0, 500.0))
            .build_ui_state(super::show, preset);
        h.run();
        h.get_by_role(egui::accesskit::Role::ComboBox).click();
        h.run();
        h.event(egui::Event::Text("openai".into()));
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(h.state().provider, mareader_settings::Provider::OpenAi);
        assert!(h.state().model.is_empty());
    }
}
