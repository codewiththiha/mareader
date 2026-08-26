//! Preset gallery: grouped rows of look thumbnails, plus save/delete.
//!
//! The parent owns composition; the specialized pieces own their controls
//! (`swatch` renders one thumbnail, `gallery` the grouped rows, `editor`
//! the save form). Preset domain logic (grouping, ids, builtins) stays in
//! `pdf-core::presets`.

mod editor;
mod gallery;
mod swatch;

use leptos::prelude::*;

use crate::state::AppState;
use pdf_core::presets::{group_presets, user_group_names};

use editor::PresetEditor;
use gallery::PresetGallery;

#[component]
pub fn PresetSection(state: AppState) -> impl IntoView {
    let groups = move || state.settings.with(|s| group_presets(&s.all_presets()));
    let existing_groups = move || state.settings.with(|s| user_group_names(&s.user_presets));

    view! {
        <PresetGallery state=state groups=groups />
        <PresetEditor state=state existing_groups=existing_groups />
    }
}
