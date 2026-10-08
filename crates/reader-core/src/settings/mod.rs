//! Persisted user settings: the field names below are the serde schema
//! in localStorage.

use serde::{Deserialize, Serialize};

use crate::appearance::Appearance;
use crate::appearance::presets::{Preset, builtin_presets};

mod animation;
mod cefr;
mod gloss;
mod layout;
mod workspace;

// The reflowable typography schema lives here; the field names are
// the storage contract.
pub mod typography;

/// The layout and animation schemas, re-exported from their own files.
pub use animation::AnimationSettings;
pub use cefr::{CefrLevel, MIN_CEFR_BAND};
pub use layout::{
    DEFAULT_COLUMN_WIDTH_PCT, FloatingLabelStyle, LayoutSettings, MAX_COLUMN_WIDTH_PCT,
    MIN_COLUMN_WIDTH_PCT, PageIndicatorStyle,
};
pub use typography::TextSettings;
/// The reader workspace's knobs (the Workspace tab).
pub use workspace::{LibraryClick, PaneCorners, PaneOutlineColor, WorkspaceSettings};

/// The AI word card's knobs, part of the persisted schema.
pub use gloss::{GlossColor, GlossDensity, default_custom_gloss, default_gloss_opacity, is_hex6};

/// Which pixels carry the paper colour; owned by `pdf-paper`.
pub use pdf_paper::PaperArea;

pub const SETTINGS_KEY: &str = "mareader.settings.v1";

/// The key this one replaced; read, never written.
pub const RETIRED_SETTINGS_KEY: &str = "pdfreader.settings.v1";

/// `serde(default)` for the flags that were on before they were a switch.
pub(crate) fn on_true() -> bool {
    true
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The live look. Edited directly by the appearance controls.
    pub appearance: Appearance,
    /// User-saved presets (built-ins are code, not storage).
    pub user_presets: Vec<Preset>,
    /// One-shot gate for the doubled tint curve.
    #[serde(default)]
    pub tint_strength_halved: bool,
    /// One-shot gate for the startup-fit default change.
    #[serde(default)]
    pub startup_fit_width: bool,
    pub default_zoom: f64,
    pub last_path: Option<String>,
    /// Pin the READER's titlebar open; one field per bar.
    #[serde(default)]
    pub titlebar_pinned: bool,
    /// Pin the LIBRARY's own titlebar; defaults pinned.
    #[serde(default = "default_library_titlebar_pinned")]
    pub library_titlebar_pinned: bool,
    #[serde(default)]
    pub layout: LayoutSettings,
    #[serde(default)]
    pub animations: AnimationSettings,
    #[serde(default)]
    pub gloss_color: GlossColor,
    #[serde(default = "default_gloss_opacity")]
    pub gloss_opacity: f64,
    #[serde(default = "default_custom_gloss")]
    pub gloss_custom: String,
    /// The AI word card's spacing; denser by default.
    #[serde(default)]
    pub gloss_density: GlossDensity,
    /// The vocabulary highlighter: words above the reader's band in red ink.
    #[serde(default = "on_true")]
    pub cefr_enabled: bool,
    /// The reader's own band; words ABOVE it are highlighted.
    #[serde(default)]
    pub cefr_level: CefrLevel,
    /// Whether clicking a red word starts an AI explanation.
    #[serde(default = "on_true")]
    pub cefr_click_explain: bool,
    /// Typography of the reflowable formats; PDFs never read it.
    #[serde(default)]
    pub text: TextSettings,
    /// The reader workspace: what a Library-panel click does.
    #[serde(default)]
    pub workspace: WorkspaceSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            user_presets: Vec::new(),
            // A fresh install is born on the new curve: nothing to migrate.
            tint_strength_halved: true,
            startup_fit_width: true,
            default_zoom: 1.0,
            last_path: None,
            titlebar_pinned: false,
            library_titlebar_pinned: default_library_titlebar_pinned(),
            layout: LayoutSettings::default(),
            animations: AnimationSettings::default(),
            gloss_color: GlossColor::default(),
            gloss_opacity: default_gloss_opacity(),
            gloss_custom: default_custom_gloss(),
            gloss_density: GlossDensity::default(),
            cefr_enabled: true,
            cefr_level: CefrLevel::default(),
            cefr_click_explain: true,
            text: TextSettings::default(),
            workspace: WorkspaceSettings::default(),
        }
    }
}

/// [`Settings::library_titlebar_pinned`]'s default, which predates the field.
fn default_library_titlebar_pinned() -> bool {
    true
}

impl Settings {
    /// Built-ins first, then the user's own — the order the menu renders in.
    pub fn all_presets(&self) -> Vec<Preset> {
        let mut v = builtin_presets();
        v.extend(self.user_presets.iter().cloned());
        v
    }

    /// Record a manual appearance edit: clamp the knobs to their ranges.
    pub fn touch_appearance(&mut self) {
        self.appearance.sanitize();
    }
}

/// The old tint curve's strength on the new one: half, rounded up.
fn halved_strength(v: u8) -> u8 {
    v.saturating_add(1) / 2
}

/// Ensures a persisted `Settings` is internally valid.
pub fn sanitize(settings: &mut Settings) {
    // The one-shot curve migration: old /100 strengths halve onto the new
    // slope.
    if !settings.tint_strength_halved {
        settings.appearance.tint_strength = halved_strength(settings.appearance.tint_strength);
        for p in settings.user_presets.iter_mut() {
            p.appearance.tint_strength = halved_strength(p.appearance.tint_strength);
        }
        settings.tint_strength_halved = true;
    }
    if !settings.startup_fit_width {
        settings.layout.default_fit = layout::default_startup_fit();
        settings.startup_fit_width = true;
    }
    settings.appearance.sanitize();
    typography::sanitize(&mut settings.text);
    settings.default_zoom = settings.default_zoom.clamp(0.25, 5.0);
    settings.gloss_opacity = settings.gloss_opacity.clamp(0.1, 1.0);
    settings.workspace.pane_outline_width = settings.workspace.pane_outline_width.min(8);
    settings.workspace.pane_gap = settings.workspace.pane_gap.min(24);
    if !is_hex6(&settings.workspace.pane_outline_custom) {
        settings.workspace.pane_outline_custom = WorkspaceSettings::default().pane_outline_custom;
    }
    settings.layout.page_margin = settings.layout.page_margin.clamp(0.0, 64.0);
    settings.layout.column_width_pct = settings
        .layout
        .column_width_pct
        .clamp(layout::MIN_COLUMN_WIDTH_PCT, layout::MAX_COLUMN_WIDTH_PCT);
    // A startup fit of `None` falls back to the default.
    if settings.layout.default_fit == crate::zoom_math::FitMode::None {
        settings.layout.default_fit = layout::default_startup_fit();
    }
    settings.layout.floating_label_max_pct =
        settings.layout.floating_label_max_pct.clamp(10.0, 100.0);
    if !is_hex6(&settings.gloss_custom) {
        settings.gloss_custom = default_custom_gloss();
    }

    // Drop presets with empty ids or ids that shadow a built-in.
    let builtin_ids: Vec<String> = builtin_presets().into_iter().map(|p| p.id).collect();
    settings.user_presets.retain(|p| {
        !p.id.trim().is_empty() && !p.name.trim().is_empty() && !builtin_ids.contains(&p.id)
    });
    for p in settings.user_presets.iter_mut() {
        p.appearance.sanitize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::BaseMode;

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
    fn user_presets_cannot_shadow_builtins_or_be_nameless() {
        let mut s = Settings {
            user_presets: vec![
                Preset {
                    id: "sepia".into(),
                    name: "Mine".into(),
                    group: String::new(),
                    appearance: Appearance::default(),
                },
                Preset {
                    id: "ok".into(),
                    name: "  ".into(),
                    group: String::new(),
                    appearance: Appearance::default(),
                },
                Preset {
                    id: "good".into(),
                    name: "Good".into(),
                    group: "G".into(),
                    appearance: Appearance::default(),
                },
            ],
            ..Settings::default()
        };
        sanitize(&mut s);
        let ids: Vec<String> = s.user_presets.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids, vec!["good".to_string()]);
    }

    #[test]
    fn missing_fields_default() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.appearance, Appearance::default());
        assert!(s.user_presets.is_empty());
    }

    #[test]
    fn an_old_blob_halves_its_tint_strengths_once() {
        // A blob written before the doubled tint curve: no gate field.
        let mut s: Settings = serde_json::from_str(
            r#"{"appearance": {"tint_hue": 34, "tint_strength": 45},
                "user_presets": [
                    {"id": "mine", "name": "Mine", "appearance": {"tint_strength": 40}}
                ]}"#,
        )
        .unwrap();
        assert!(
            !s.tint_strength_halved,
            "a blob without the gate loads un-migrated"
        );
        sanitize(&mut s);
        assert_eq!(
            s.appearance.tint_strength, 23,
            "45 on the old slope is the Sepia preset's own 23 on the new one"
        );
        assert_eq!(s.user_presets[0].appearance.tint_strength, 20);
        assert!(s.tint_strength_halved);
        // Once means once: sanitizing runs on load and on every write.
        sanitize(&mut s);
        assert_eq!(s.appearance.tint_strength, 23);
    }

    #[test]
    fn fresh_settings_are_born_on_the_new_curve() {
        // The in-memory default must not look like an old blob.
        let mut s = Settings::default();
        s.appearance.tint_strength = 45;
        sanitize(&mut s);
        assert_eq!(
            s.appearance.tint_strength, 45,
            "nothing to migrate on a fresh blob"
        );
        assert!(s.tint_strength_halved);
    }

    #[test]
    fn layout_settings_default() {
        let s = LayoutSettings::default();
        assert_eq!(s.page_margin, 0.0);
        assert!(s.auto_scale);
        assert!(s.auto_resize);
        assert!(s.page_shadow);
        assert!(!s.sidebar_overlay);
        assert!(!s.blend_mode);
        assert_eq!(s.blend_area, PaperArea::WholePage);
        assert!(!s.floating_label_persist);
        assert_eq!(s.floating_label_max_pct, 100.0);
        // Startup fit defaults to Fit Width.
        assert_eq!(s.default_fit, crate::zoom_math::FitMode::Width);

        // A blob saved before `auto_resize` existed has exactly this shape.
        let s: LayoutSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.page_margin, 0.0);
        assert!(s.auto_scale);
        assert!(s.auto_resize);
        assert!(s.page_shadow);
        assert!(!s.sidebar_overlay);
        assert!(!s.blend_mode);
        assert_eq!(s.blend_area, PaperArea::WholePage);
        assert_eq!(s.default_fit, crate::zoom_math::FitMode::Width);
        assert!(!s.floating_label_persist);
        assert_eq!(s.floating_label_max_pct, 100.0);
    }

    #[test]
    fn a_startup_fit_of_none_is_reset_to_width() {
        let mut s = Settings::default();
        s.layout.default_fit = crate::zoom_math::FitMode::None;
        sanitize(&mut s);
        assert_eq!(s.layout.default_fit, crate::zoom_math::FitMode::Width);
    }

    #[test]
    fn an_old_startup_fit_moves_to_width_once() {
        let mut s = Settings {
            startup_fit_width: false,
            layout: LayoutSettings {
                default_fit: crate::zoom_math::FitMode::Page,
                ..LayoutSettings::default()
            },
            ..Settings::default()
        };
        sanitize(&mut s);
        assert_eq!(s.layout.default_fit, crate::zoom_math::FitMode::Width);
        assert!(s.startup_fit_width);
        // A later explicit choice survives every subsequent sanitize.
        s.layout.default_fit = crate::zoom_math::FitMode::Page;
        sanitize(&mut s);
        assert_eq!(s.layout.default_fit, crate::zoom_math::FitMode::Page);
    }

    #[test]
    fn the_detection_area_round_trips() {
        let s = LayoutSettings {
            blend_area: PaperArea::Edges,
            ..LayoutSettings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"blend_area\":\"edges\""), "{json}");
        let back: LayoutSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.blend_area, PaperArea::Edges);
    }

    #[test]
    fn a_blob_from_the_fixed_mode_era_still_loads() {
        // Older blobs carry a paper mode and a scan budget; both are gone.
        let s: LayoutSettings = serde_json::from_str(
            r#"{"blend_mode":true,"blend_scope":"fixed","blend_area":"edges","blend_scan_pages":100}"#,
        )
        .unwrap();
        assert!(s.blend_mode);
        assert_eq!(s.blend_area, PaperArea::Edges);
    }

    #[test]
    fn every_animation_is_on_until_told_otherwise() {
        let a = AnimationSettings::default();
        assert!(a.enabled);
        assert!(a.sidebar_slide && a.canvas_resize);
        assert!(a.zoom && a.scroll_jumps);

        // A blob saved before this group existed deserialises like `{}`.
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(s.animations.enabled && s.animations.zoom);
        // A half-written group defaults the fields it does not carry.
        let a: AnimationSettings = serde_json::from_str(r#"{"enabled":false}"#).unwrap();
        assert!(!a.enabled);
        assert!(a.zoom && a.sidebar_slide && a.canvas_resize);
    }

    #[test]
    fn label_width_limit_is_clamped() {
        let mut s = Settings::default();
        s.layout.floating_label_max_pct = 420.0;
        sanitize(&mut s);
        assert_eq!(s.layout.floating_label_max_pct, 100.0);

        s.layout.floating_label_max_pct = 0.0;
        sanitize(&mut s);
        assert_eq!(s.layout.floating_label_max_pct, 10.0);
    }

    #[test]
    fn the_column_width_dial_is_clamped() {
        let mut s = Settings::default();
        assert_eq!(s.layout.column_width_pct, DEFAULT_COLUMN_WIDTH_PCT);

        s.layout.column_width_pct = 400.0;
        sanitize(&mut s);
        assert_eq!(s.layout.column_width_pct, MAX_COLUMN_WIDTH_PCT);

        s.layout.column_width_pct = 5.0;
        sanitize(&mut s);
        assert_eq!(s.layout.column_width_pct, MIN_COLUMN_WIDTH_PCT);
    }
}
