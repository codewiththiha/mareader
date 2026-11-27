//! A searchable choice of saved word-list presets.

use egui::Ui;
use mareader_settings::WordPreset;

pub(super) fn pick(
    ui: &mut Ui,
    presets: &[WordPreset],
    instant: &Option<String>,
    current: &Option<String>,
) -> Option<WordPreset> {
    let shown = current.clone().unwrap_or_else(|| "Custom".into());
    let mut selected = current.clone();
    let items: Vec<_> = presets
        .iter()
        .map(|p| {
            let star = if instant.as_ref() == Some(&p.name) {
                " ★"
            } else {
                ""
            };
            (Some(p.name.clone()), format!("{}{star}", p.name))
        })
        .collect();
    let response = crate::dropdown::select(
        ui,
        egui::Id::new("word-list-preset"),
        shown,
        ui.available_width(),
        &mut selected,
        &items,
    );
    if !response.changed() {
        return None;
    }
    selected.and_then(|name| presets.iter().find(|p| p.name == name).cloned())
}
