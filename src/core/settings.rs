//! Persisted user settings.
//!
//! CONTRACT: field names below are the serde schema persisted to localStorage
//! under `pdfreader.settings.v1`. Do not rename fields.
//!
//! SCHEMA EVOLUTION. The appearance model changed shape (six fixed themes ->
//! base mode + computed tint + presets), but the storage KEY did not: bumping
//! it to `.v2` would silently reset everyone's last-opened file and zoom too.
//! Instead the old fields are kept as `Option`s and migrated on load, so an
//! existing install lands on the preset that reproduces the theme it had. The
//! old fields are dropped when writing, so the migration runs at most once.

use serde::{Deserialize, Serialize};

use crate::core::appearance::{Appearance, NoiseMode, TextureMode};
use crate::core::presets::{builtin_presets, Preset};

pub const SETTINGS_KEY: &str = "pdfreader.settings.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The live look. Edited directly by the appearance controls.
    pub appearance: Appearance,
    /// Id of the preset currently selected, if the live look still matches it.
    /// Cleared as soon as the user nudges any slider, which is what lets the
    /// menu show "Custom" honestly instead of claiming a preset is active when
    /// it has been modified.
    pub active_preset: Option<String>,
    /// User-saved presets (built-ins are code, not storage).
    pub user_presets: Vec<Preset>,
    pub default_zoom: f64,
    pub last_path: Option<String>,

    // --- legacy fields, read once then dropped -------------------------------
    #[serde(skip_serializing, default)]
    pub theme_id: Option<String>,
    #[serde(skip_serializing, default)]
    pub texture: Option<TextureMode>,
    #[serde(skip_serializing, default)]
    pub noise_enabled: Option<bool>,
    #[serde(skip_serializing, default)]
    pub noise_intensity: Option<u8>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            active_preset: Some("light".to_string()),
            user_presets: Vec::new(),
            default_zoom: 1.0,
            last_path: None,
            theme_id: None,
            texture: None,
            noise_enabled: None,
            noise_intensity: None,
        }
    }
}

impl Settings {
    /// Built-ins first, then the user's own — the order the menu renders in.
    pub fn all_presets(&self) -> Vec<Preset> {
        let mut v = builtin_presets();
        v.extend(self.user_presets.iter().cloned());
        v
    }

    pub fn find_preset(&self, id: &str) -> Option<Preset> {
        self.all_presets().into_iter().find(|p| p.id == id)
    }

    /// Apply a preset: copy its look and remember which one is active.
    pub fn apply_preset(&mut self, id: &str) {
        if let Some(p) = self.find_preset(id) {
            self.appearance = p.appearance;
            self.active_preset = Some(p.id);
        }
    }

    /// Record a manual appearance edit. Any hand edit detaches from the
    /// preset UNLESS it happens to land back exactly on it.
    pub fn touch_appearance(&mut self) {
        self.appearance.sanitize();
        let still = self
            .active_preset
            .as_ref()
            .and_then(|id| self.find_preset(id))
            .map(|p| p.appearance == self.appearance)
            .unwrap_or(false);
        if !still {
            self.active_preset = self
                .all_presets()
                .into_iter()
                .find(|p| p.appearance == self.appearance)
                .map(|p| p.id);
        }
    }
}

/// Map a retired theme id onto the preset that reproduces it.
fn legacy_theme_to_preset(id: &str) -> &'static str {
    match id {
        "dark" => "dark",
        "dim" => "dim",
        "sepia" => "sepia",
        "green" => "green",
        "night" => "night",
        _ => "light",
    }
}

/// Ensures a persisted `Settings` is internally valid, and migrates the
/// pre-preset schema.
pub fn sanitize(settings: &mut Settings) {
    // --- migration -----------------------------------------------------------
    // Presence of a legacy `theme_id` means this blob predates the appearance
    // model. Rebuild the look from it so the user's chosen theme survives the
    // upgrade instead of snapping back to Light.
    if let Some(old) = settings.theme_id.take() {
        let id = legacy_theme_to_preset(&old);
        if let Some(p) = builtin_presets().into_iter().find(|p| p.id == id) {
            settings.appearance = p.appearance;
            settings.active_preset = Some(p.id);
        }
        // Texture and grain were independent of the theme before, so carry them
        // across on top of the reconstructed look rather than letting the
        // preset's own values overwrite what the user had set.
        if let Some(t) = settings.texture.take() {
            settings.appearance.texture = t;
        }
        if let Some(on) = settings.noise_enabled.take() {
            settings.appearance.noise = if on { NoiseMode::Static } else { NoiseMode::Off };
        }
        if let Some(i) = settings.noise_intensity.take() {
            settings.appearance.noise_intensity = i;
        }
        // Those carried-over values almost certainly no longer match the
        // preset, so re-derive whether one is really active.
        settings.touch_appearance();
    }
    settings.texture = None;
    settings.noise_enabled = None;
    settings.noise_intensity = None;

    // --- validation ----------------------------------------------------------
    settings.appearance.sanitize();
    settings.default_zoom = settings.default_zoom.clamp(0.25, 5.0);

    // Drop user presets with empty ids/names or ids that shadow a built-in;
    // both would make rows unselectable in the menu.
    let builtin_ids: Vec<String> = builtin_presets().into_iter().map(|p| p.id).collect();
    settings.user_presets.retain(|p| {
        !p.id.trim().is_empty() && !p.name.trim().is_empty() && !builtin_ids.contains(&p.id)
    });
    for p in settings.user_presets.iter_mut() {
        p.appearance.sanitize();
    }

    // A dangling active_preset (deleted preset) must not leave the menu
    // highlighting nothing while claiming a selection.
    if let Some(id) = settings.active_preset.clone() {
        if !settings.all_presets().iter().any(|p| p.id == id) {
            settings.active_preset = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::appearance::BaseMode;

    #[test]
    fn settings_round_trip() {
        let mut s = Settings::default();
        s.appearance.base = BaseMode::Dark;
        s.appearance.tint_hue = 200;
        s.appearance.tint_strength = 40;
        s.default_zoom = 1.25;
        s.last_path = Some("/tmp/a.pdf".to_string());
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn legacy_themes_migrate_to_the_matching_preset() {
        // The upgrade path that matters: someone reading in Sepia must still be
        // in Sepia after the update, not thrown back to Light.
        for (old, want) in [
            ("sepia", "sepia"),
            ("green", "green"),
            ("night", "night"),
            ("dark", "dark"),
            ("dim", "dim"),
            ("light", "light"),
        ] {
            let json = format!(r#"{{"theme_id":"{old}"}}"#);
            let mut s: Settings = serde_json::from_str(&json).unwrap();
            sanitize(&mut s);
            assert_eq!(s.active_preset.as_deref(), Some(want), "migrating {old}");
        }
    }

    #[test]
    fn migration_reconstructs_the_actual_look_not_just_the_id() {
        let mut s: Settings = serde_json::from_str(r#"{"theme_id":"night"}"#).unwrap();
        sanitize(&mut s);
        assert_eq!(s.appearance.base, BaseMode::Dark);
        assert!(s.appearance.has_tint(), "Night had a green cast");
    }

    #[test]
    fn migration_carries_texture_and_grain_across() {
        // Texture/noise were independent of the theme, so they must survive the
        // move even though the preset they land on has its own values.
        let json = r#"{"theme_id":"sepia","texture":"lined","noise_enabled":true,"noise_intensity":70}"#;
        let mut s: Settings = serde_json::from_str(json).unwrap();
        sanitize(&mut s);
        assert_eq!(s.appearance.texture, TextureMode::Lined);
        assert_eq!(s.appearance.noise, NoiseMode::Static);
        assert_eq!(s.appearance.noise_intensity, 70);
        // Sepia + a lined texture is no longer the stock Sepia preset.
        assert_eq!(s.active_preset, None, "modified look must read as Custom");
    }

    #[test]
    fn legacy_fields_are_not_written_back() {
        let mut s: Settings = serde_json::from_str(r#"{"theme_id":"sepia"}"#).unwrap();
        sanitize(&mut s);
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("theme_id"), "migration must run once: {json}");
        // ...and re-reading keeps the migrated look.
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.active_preset.as_deref(), Some("sepia"));
    }

    #[test]
    fn applying_a_preset_sets_both_look_and_selection() {
        let mut s = Settings::default();
        s.apply_preset("green");
        assert_eq!(s.active_preset.as_deref(), Some("green"));
        assert_eq!(s.appearance.tint_hue, 104);
    }

    #[test]
    fn editing_a_slider_detaches_from_the_preset() {
        let mut s = Settings::default();
        s.apply_preset("sepia");
        s.appearance.tint_hue = 210;
        s.touch_appearance();
        assert_eq!(s.active_preset, None, "an edited preset is no longer that preset");
    }

    #[test]
    fn editing_back_onto_a_preset_reselects_it() {
        // Nice-to-have that avoids a lying UI: if you dial the sliders to
        // exactly Green, the menu should say Green.
        let mut s = Settings::default();
        s.apply_preset("light");
        s.appearance = builtin_presets().into_iter().find(|p| p.id == "green").unwrap().appearance;
        s.touch_appearance();
        assert_eq!(s.active_preset.as_deref(), Some("green"));
    }

    #[test]
    fn user_presets_cannot_shadow_builtins_or_be_nameless() {
        let mut s = Settings::default();
        s.user_presets = vec![
            Preset { id: "sepia".into(), name: "Mine".into(), group: String::new(), appearance: Appearance::default() },
            Preset { id: "ok".into(), name: "  ".into(), group: String::new(), appearance: Appearance::default() },
            Preset { id: "good".into(), name: "Good".into(), group: "G".into(), appearance: Appearance::default() },
        ];
        sanitize(&mut s);
        let ids: Vec<String> = s.user_presets.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids, vec!["good".to_string()]);
    }

    #[test]
    fn a_deleted_active_preset_does_not_dangle() {
        let mut s = Settings::default();
        s.active_preset = Some("gone".to_string());
        sanitize(&mut s);
        assert_eq!(s.active_preset, None);
    }

    #[test]
    fn missing_fields_default() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.appearance, Appearance::default());
        assert!(s.user_presets.is_empty());
    }
}
