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

use crate::appearance::{Appearance, BaseMode, NoiseMode, TextureMode};
use crate::presets::{builtin_presets, Preset};

pub const SETTINGS_KEY: &str = "pdfreader.settings.v1";

fn on_true() -> bool { true }
fn default_gloss_opacity() -> f64 { 0.4 }
fn default_page_margin() -> f64 { 0.0 }
fn default_label_max_pct() -> f64 { 100.0 }
fn default_custom_gloss() -> String { "#a58af0".into() }
fn default_startup_fit() -> crate::math::FitMode {
    crate::math::FitMode::Page
}

fn is_hex6(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 7 && b[0] == b'#' && b[1..].iter().all(|c| c.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageIndicatorStyle {
    #[default]
    PageNumber,
    Percentage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatingLabelStyle {
    #[default]
    #[serde(alias = "title")] // migration: old saved "title" loads as FileName
    FileName,
    Chapter,
}

/// The blend backdrop's paper pipeline is configured by the `pdf-paper`
/// crate's types: [`PaperMode`] (one fixed colour for the book, or a
/// per-page palette that follows the scroll), [`PaperArea`] (which pixels
/// of a page carry the colour — the whole raster or just its edge
/// margins) and the scan-page budget. They are re-exported here so the
/// settings model stays the one place the reader's persisted knobs live.
pub use pdf_paper::{PaperArea, PaperMode};

fn default_blend_scan_pages() -> u32 {
    pdf_paper::DEFAULT_SCAN_PAGES
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayoutSettings {
    #[serde(default = "on_true")]
    pub page_indicator: bool,
    #[serde(default)]
    pub page_indicator_style: PageIndicatorStyle,
    #[serde(default = "on_true")]
    pub floating_label: bool,
    #[serde(default)]
    pub floating_label_style: FloatingLabelStyle,
    #[serde(default = "on_true")]
    pub progress_bar: bool,
    /// The fit mode applied when a document opens (or a book resumes): how the
    /// reader sizes the first page. `FitMode::Page` fits the whole page; `Width`
    /// fits the width. `None` is not a valid startup mode.
    #[serde(default = "default_startup_fit")]
    pub default_fit: crate::math::FitMode,
    /// Remove the vertical gap between pages in scroll view.
    #[serde(default)]
    pub no_gap: bool,
    #[serde(default = "on_true")]
    pub auto_scale: bool,
    /// Let a page turn change the scale. On, arriving at a page whose size
    /// differs re-resolves the zoom: an active fit re-fits, and a hand-picked
    /// zoom is held to the new page's fit-width ceiling. Off, a page turn
    /// touches nothing — the zoom, the scroll position and the measured column
    /// all stay where the reader put them, and a page wider than the window
    /// overflows and scrolls. The WINDOW still re-fits either way; this
    /// switch is only about the page under the eyes.
    #[serde(default = "on_true")]
    pub auto_resize: bool,
    #[serde(default = "on_true")]
    pub page_shadow: bool,
    #[serde(default)]
    pub sidebar_overlay: bool,
    #[serde(default)]
    pub blend_mode: bool,
    /// Which pages blend mode takes the paper colour from (one fixed colour
    /// for the book, or a per-page palette that follows the scroll). Only
    /// meaningful while `blend_mode` is on; the setting outlives the switch
    /// so turning blend back on returns to the mode the reader chose. The
    /// field keeps its historical name so pre-crate blobs — whose values
    /// were `single`/`document`/`continuous` — load through `PaperMode`'s
    /// serde aliases: the first two fold into `Fixed`.
    #[serde(default)]
    pub blend_scope: PaperMode,
    /// Which pixels of a page raster the detector trusts: the whole page,
    /// or just the thin left/right edge margins where artwork-heavy pages
    /// still show honest paper.
    #[serde(default)]
    pub blend_area: PaperArea,
    /// The fixed-mode scan's page budget — at most this many pages are
    /// sampled for the book's one colour. 100 by default.
    #[serde(default = "default_blend_scan_pages")]
    pub blend_scan_pages: u32,
    /// Horizontal inset around pages (CSS px). `0` removes the margin entirely.
    #[serde(default = "default_page_margin")]
    pub page_margin: f64,
    /// Keep the floating label on screen even when the sidebar or title bar
    /// would normally hide it, and ignore the width budget.
    #[serde(default)]
    pub floating_label_persist: bool,
    /// Share of the measured width budget (in percent) the floating label may
    /// consume before it fades out. `100` hides it only on a true overflow.
    #[serde(default = "default_label_max_pct")]
    pub floating_label_max_pct: f64,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            page_indicator: true,
            page_indicator_style: PageIndicatorStyle::PageNumber,
            floating_label: true,
            floating_label_style: FloatingLabelStyle::FileName,
            progress_bar: true,
            default_fit: default_startup_fit(),
            no_gap: false,
            auto_scale: true,
            auto_resize: true,
            page_shadow: true,
            sidebar_overlay: false,
            blend_mode: false,
            blend_scope: PaperMode::default(),
            blend_area: PaperArea::default(),
            blend_scan_pages: default_blend_scan_pages(),
            page_margin: default_page_margin(),
            floating_label_persist: false,
            floating_label_max_pct: default_label_max_pct(),
        }
    }
}

/// What may animate, and what may not.
///
/// `enabled` is the master, and it lives in the Layout tab because it is a
/// layout-scale decision: the reader either moves or it does not. The detail
/// switches are the Animations tab, and that tab — like every animation here —
/// only exists while the master is on. The projection that applies the master is
/// `Motion::from_prefs` in the app crate, so no consumer has to remember to ask
/// twice.
///
/// Turning a switch off never skips the change itself. It renders the END frame
/// instantly: the zoom still lands at its target, the page still gets the width
/// the new space allows, the column still arrives where it was sent — with no
/// frames in between. That is what makes the whole group safe to freeze:
/// nothing is lost, only interpolated.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnimationSettings {
    /// Every animation in the reader, including the ones with no switch of
    /// their own (menu pops, toasts, the theme cross-fade, the title bar's
    /// auto-hide). Off, the Animations tab is not offered either.
    #[serde(default = "on_true")]
    pub enabled: bool,
    /// The rail animates its open and close instead of appearing at the end
    /// state — the DOCKED rail tweens its width, the FLOATING rail fades in
    /// and out (a transform slide would travel under the native traffic
    /// lights, which can only appear and disappear). The page ALWAYS rides
    /// the docked tween — there is no switch for it, because following a
    /// measured container is not an animation: it is the resize. Deferring it
    /// would show a page cropped at the old scale for the whole settle window,
    /// which is worse than the slide itself; and with the animation off,
    /// answering in the frame the rail moved is what makes ONE step out of
    /// the whole change. A WINDOW drag is the burst with a switch of its own,
    /// below: there, skipping frames is the point, because the alternative is
    /// a relayout plus a raster per drag frame.
    #[serde(default = "on_true")]
    pub sidebar_slide: bool,
    /// The page re-fits on every frame of a window drag. Off, it re-fits once,
    /// when the drag stops.
    #[serde(default = "on_true")]
    pub canvas_resize: bool,
    /// A zoom eases to its target instead of appearing there.
    #[serde(default = "on_true")]
    pub zoom: bool,
    /// Jumping to a page (or to a search hit) glides the column there.
    #[serde(default = "on_true")]
    pub scroll_jumps: bool,
}

impl Default for AnimationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sidebar_slide: true,
            canvas_resize: true,
            zoom: true,
            scroll_jumps: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlossColor {
    #[default]
    Accent,
    Red,
    Yellow,
    Green,
    Blue,
    /// Old saved `"violet"` becomes Custom (default hex = old violet).
    #[serde(alias = "violet")]
    Custom,
}

impl GlossColor {
    pub fn all() -> [GlossColor; 6] {
        [Self::Accent, Self::Red, Self::Yellow, Self::Green, Self::Blue, Self::Custom]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Accent => "Auto",
            Self::Red => "Red",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Custom => "Custom",
        }
    }

    /// `None` = follow the live accent tint.
    pub fn resolve(&self, custom: &str) -> Option<String> {
        match self {
            Self::Accent => None,
            Self::Red => Some("#e56b64".into()),
            Self::Yellow => Some("#e8c449".into()),
            Self::Green => Some("#6fd58c".into()),
            Self::Blue => Some("#6ba3f5".into()),
            Self::Custom => Some(custom.to_string()),
        }
    }
}

/// How much air the AI word card carries: the padding, line heights and
/// section gaps of the gloss card's body. Compact is the default because a
/// definition is scanned, not read like a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GlossDensity {
    #[default]
    Compact,
    Comfortable,
}

impl GlossDensity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Comfortable => "Comfortable",
        }
    }
}


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
    /// Pin the titlebar open (no auto-hide). Persisted; `serde(default)`
    /// migrates pre-pin blobs to unpinned.
    #[serde(default)]
    pub titlebar_pinned: bool,
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
    /// The AI word card's spacing. Blobs saved before the field existed
    /// deserialize as Compact — the card had grown visibly airy and the
    /// denser layout is the better default even for readers who never open
    /// Settings.
    #[serde(default)]
    pub gloss_density: GlossDensity,

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
            // No preset matches a fresh install's plain Light look: the bases
            // are the Mode section's buttons, not presets, so "custom" is the
            // honest reading of the default state.
            active_preset: None,
            user_presets: Vec::new(),
            default_zoom: 1.0,
            last_path: None,
            titlebar_pinned: false,
            layout: LayoutSettings::default(),
            animations: AnimationSettings::default(),
            gloss_color: GlossColor::default(),
            gloss_opacity: default_gloss_opacity(),
            gloss_custom: default_custom_gloss(),
            gloss_density: GlossDensity::default(),
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

/// The preset that reproduces a retired theme id, for the themes that
/// became presets. The plain bases (light / dark / dim) answer `None` —
/// they are the Mode section's buttons now, and their migration is
/// [`legacy_theme_base`]'s job instead.
fn legacy_theme_to_preset(id: &str) -> Option<&'static str> {
    match id {
        "sepia" => Some("sepia"),
        "green" => Some("green"),
        "night" => Some("night"),
        _ => None,
    }
}

/// The base a retired plain theme id stood for. `None` for "light" (the
/// default base already IS light) and for anything the model never knew.
fn legacy_theme_base(id: &str) -> Option<BaseMode> {
    match id {
        "dark" => Some(BaseMode::Dark),
        "dim" => Some(BaseMode::Dim),
        _ => None,
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
        if let Some(id) = legacy_theme_to_preset(&old)
            && let Some(p) = builtin_presets().into_iter().find(|p| p.id == id)
        {
            settings.appearance = p.appearance;
            settings.active_preset = Some(p.id);
        } else if let Some(base) = legacy_theme_base(&old) {
            // A plain base is not a preset: restore the look and leave the
            // selection empty, which the Mode section's buttons express
            // better than a swatch ever did.
            settings.appearance = Appearance { base, ..Default::default() };
            settings.active_preset = None;
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
    settings.gloss_opacity = settings.gloss_opacity.clamp(0.1, 1.0);
    settings.layout.page_margin = settings.layout.page_margin.clamp(0.0, 64.0);
    // A startup fit of `None` is meaningless (the reader would not know how to
    // size the first page); retry to the default `FitMode::Page`.
    if settings.layout.default_fit == crate::math::FitMode::None {
        settings.layout.default_fit = default_startup_fit();
    }
    settings.layout.floating_label_max_pct =
        settings.layout.floating_label_max_pct.clamp(10.0, 100.0);
    // The paper knobs clamp through the crate's own bounds so the scan
    // budget and the edge-strip width share one definition of legal.
    settings.layout.blend_scan_pages = settings
        .layout
        .blend_scan_pages
        .clamp(pdf_paper::MIN_SCAN_PAGES, pdf_paper::MAX_SCAN_PAGES);
    if !is_hex6(&settings.gloss_custom) {
        settings.gloss_custom = default_custom_gloss();
    }

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
    if let Some(id) = settings.active_preset.clone()
        && !settings.all_presets().iter().any(|p| p.id == id)
    {
        settings.active_preset = None;
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
    fn legacy_themes_migrate_to_the_matching_preset() {
        // The upgrade path that matters: someone reading in Sepia must still be
        // in Sepia after the update, not thrown back to Light.
        for old in ["sepia", "green", "night"] {
            let json = format!(r#"{{"theme_id":"{old}"}}"#);
            let mut s: Settings = serde_json::from_str(&json).unwrap();
            sanitize(&mut s);
            assert_eq!(s.active_preset.as_deref(), Some(old), "migrating {old}");
        }
    }

    #[test]
    fn legacy_plain_themes_migrate_to_their_base_not_a_preset() {
        // Light/Dark/Dim were the plain bases; they are the Mode section's
        // buttons now, so the look survives as a bare base with no preset
        // claiming it. "light" is the default base already and needs no
        // branch at all.
        for old in ["dark", "dim"] {
            let json = format!(r#"{{"theme_id":"{old}"}}"#);
            let mut s: Settings = serde_json::from_str(&json).unwrap();
            sanitize(&mut s);
            assert_eq!(s.active_preset, None, "migrating {old}");
            let want = if old == "dark" { BaseMode::Dark } else { BaseMode::Dim };
            assert_eq!(s.appearance.base, want, "migrating {old}");
        }
        let mut s: Settings = serde_json::from_str(r#"{"theme_id":"light"}"#).unwrap();
        sanitize(&mut s);
        assert_eq!(s.appearance.base, BaseMode::Light);
        assert_eq!(s.active_preset, None);
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
        s.apply_preset("sepia");
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
    fn a_stale_plain_base_selection_is_dropped_not_dangled() {
        // Settings persisted while Light/Dark/Dim were presets carry their
        // ids as `active_preset`; the sanitizer must clear the selection
        // (the look itself lives in `appearance` and survives untouched)
        // rather than leave the menu highlighting a swatch that no longer
        // exists.
        let mut s = Settings::default();
        s.active_preset = Some("light".to_string());
        sanitize(&mut s);
        assert_eq!(s.active_preset, None);
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

    #[test]
    fn layout_settings_default() {
        let s = LayoutSettings::default();
        assert_eq!(s.page_margin, 0.0);
        assert!(s.auto_scale);
        assert!(s.auto_resize);
        assert!(s.page_shadow);
        assert!(!s.sidebar_overlay);
        assert!(!s.blend_mode);
        assert_eq!(s.blend_scope, PaperMode::Fixed);
        assert!(!s.floating_label_persist);
        assert_eq!(s.floating_label_max_pct, 100.0);
        // Startup fit defaults to Fit Page.
        assert_eq!(s.default_fit, crate::math::FitMode::Page);

        // Deserializing empty JSON layout object fills in the defaults. A
        // blob saved BEFORE `auto_resize` existed is exactly this shape, so
        // the assertion below is also the promise that an existing install
        // keeps the behaviour it had rather than losing its refit.
        let s: LayoutSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.page_margin, 0.0);
        assert!(s.auto_scale);
        assert!(s.auto_resize);
        assert!(s.page_shadow);
        assert!(!s.sidebar_overlay);
        assert!(!s.blend_mode);
        assert_eq!(s.blend_scope, PaperMode::Fixed);
        assert_eq!(s.blend_area, PaperArea::WholePage);
        assert_eq!(s.blend_scan_pages, pdf_paper::DEFAULT_SCAN_PAGES);
        assert_eq!(s.default_fit, crate::math::FitMode::Page);
        assert!(!s.floating_label_persist);
        assert_eq!(s.floating_label_max_pct, 100.0);
    }

    #[test]
    fn a_startup_fit_of_none_is_reset_to_page() {
        let mut s = Settings::default();
        s.layout.default_fit = crate::math::FitMode::None;
        sanitize(&mut s);
        assert_eq!(s.layout.default_fit, crate::math::FitMode::Page);
    }

    #[test]
    fn paper_mode_survives_a_round_trip_and_the_legacy_scopes_fold_in() {
        // A blob saved before the setting existed carries no key and must
        // load as the fixed mode; once the reader picks a mode, the id
        // round-trips untouched. The pre-crate scopes (`single`,
        // `document`) both load as Fixed — their union — so no install
        // loses its backdrop on the upgrade.
        let s: LayoutSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.blend_scope, PaperMode::Fixed);

        let mut s = LayoutSettings::default();
        s.blend_scope = PaperMode::Continuous;
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"blend_scope\":\"continuous\""), "{json}");
        let back: LayoutSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.blend_scope, PaperMode::Continuous);

        for old in ["single", "document"] {
            let s: LayoutSettings =
                serde_json::from_str(&format!(r#"{{"blend_scope":"{old}"}}"#))
                    .unwrap();
            assert_eq!(s.blend_scope, PaperMode::Fixed, "scope {old}");
        }
    }

    #[test]
    fn the_area_and_scan_budget_round_trip_and_clamp() {
        let mut s = LayoutSettings::default();
        s.blend_area = PaperArea::Edges;
        s.blend_scan_pages = 250;
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"blend_area\":\"edges\""), "{json}");
        assert!(json.contains("\"blend_scan_pages\":250"), "{json}");
        let back: LayoutSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.blend_area, PaperArea::Edges);
        assert_eq!(back.blend_scan_pages, 250);

        s.blend_scan_pages = 0;
        let mut whole = Settings {
            layout: s,
            ..Settings::default()
        };
        sanitize(&mut whole);
        assert_eq!(whole.layout.blend_scan_pages, pdf_paper::MIN_SCAN_PAGES);
        whole.layout.blend_scan_pages = 10_000;
        sanitize(&mut whole);
        assert_eq!(whole.layout.blend_scan_pages, pdf_paper::MAX_SCAN_PAGES);
    }

    #[test]
    fn every_animation_is_on_until_told_otherwise() {
        let a = AnimationSettings::default();
        assert!(a.enabled);
        assert!(a.sidebar_slide && a.canvas_resize);
        assert!(a.zoom && a.scroll_jumps);

        // A blob saved before this group existed is `Settings` with the key
        // missing, so it deserialises exactly like `{}`: the reader must keep
        // animating across an update rather than freeze.
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(s.animations.enabled && s.animations.zoom);
        // A half-written group defaults the fields it does not carry, one by
        // one — a stored master-off must not silently turn the details on.
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
}
