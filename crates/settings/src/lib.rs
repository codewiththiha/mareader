//! Every user setting as typed data, with ranges and loading (L2).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod ai;
mod ai_job;
mod load;
mod reader;
mod study;
mod swatch;
mod text;
mod words;
mod workspace;

use mareader_appearance::{Appearance, Preset, builtin_presets, preset_id};
use mareader_library::View;
use serde::{Deserialize, Serialize};

pub use ai::{
    AiSettings, AiTasks, Endpoint, ExplainTask, LANGUAGES, ModelPreset, OrganizeTask, PageRun,
    PageStart, PageTask, PresetId, Provider, SimplifyTask, SummarizeTask, SummaryStyle,
    TranslateTask, language_name,
};
pub use ai_job::Job;
pub use load::{Loaded, load, to_json};
pub use reader::{
    AppearanceMenuStyle, Bar, ChromeSettings, FloatingLabelStyle, Highlight, LayoutSettings,
    MotionSettings, PageIndicatorStyle, PaperArea, PaperSettings, Pass, Player, ReadAloudSettings,
    Skip, Skips, StartupFit,
};
pub use study::StudySettings;
pub use study::{
    Accent, BarItem, CefrLevel, CefrMark, CefrSettings, DictionaryOpens, DictionarySettings,
    DictionaryWindow, GlossDensity, GlossSettings, HighlightSettings, Notation, Pen, PickColor,
    PronunciationSettings, Reserved, WINDOW_MIN,
};
pub use swatch::Swatch;
pub use text::{BookDesign, ColumnAlign, FontChoice, FontFamily, SystemFont, TextSettings};
pub use words::{
    WORD_LANGUAGES, WORD_PRESETS, WordFields, WordGroup, WordLayout, WordList, WordListSettings,
    WordOutput, WordPreset, WordSort,
};
pub use workspace::{Corners, EdgeParking, LibraryClick, PaneLook, WorkspaceSettings};

/// Bumped with every migration step in `load.rs`.
pub const SCHEMA_VERSION: u32 = 7;

/// The whole persisted state of the user's choices.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub appearance: Appearance,
    /// The user's own presets; the built-ins live in code.
    pub user_presets: Vec<Preset>,
    pub chrome: ChromeSettings,
    pub layout: LayoutSettings,
    pub paper: PaperSettings,
    pub motion: MotionSettings,
    /// EPUB typography; PDFs never read it.
    pub text: TextSettings,
    pub study: StudySettings,
    pub dictionary: DictionarySettings,
    /// Named models and the tasks that use them; keys live apart.
    pub ai: AiSettings,
    pub workspace: WorkspaceSettings,
    /// The library page's layout and order.
    pub library: View,
    pub read_aloud: ReadAloudSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            appearance: Appearance::default(),
            user_presets: Vec::new(),
            chrome: ChromeSettings::default(),
            layout: LayoutSettings::default(),
            paper: PaperSettings::default(),
            motion: MotionSettings::default(),
            text: TextSettings::default(),
            study: StudySettings::default(),
            dictionary: DictionarySettings::default(),
            ai: AiSettings::default(),
            workspace: WorkspaceSettings::default(),
            library: View::default(),
            read_aloud: ReadAloudSettings::default(),
        }
    }
}

impl Settings {
    /// Pulls every value into its range; runs on load and on every edit.
    pub fn sanitize(&mut self) {
        self.version = SCHEMA_VERSION;
        self.appearance.sanitize();
        self.layout.sanitize();
        self.text.sanitize();
        self.study.sanitize();
        self.dictionary.sanitize();
        self.ai.sanitize();
        self.workspace.look.sanitize();
        self.library.sanitize();
        self.read_aloud.sanitize();
        let builtin: Vec<String> = builtin_presets().into_iter().map(|p| p.id).collect();
        self.user_presets.retain(|p| {
            !p.id.trim().is_empty() && !p.name.trim().is_empty() && !builtin.contains(&p.id)
        });
        for p in &mut self.user_presets {
            p.appearance.sanitize();
        }
    }

    /// Keeps `look` as a preset; a blank name saves nothing.
    pub fn save_preset(&mut self, name: &str, group: &str, look: Appearance) -> Option<String> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        let id = preset_id(name, &self.user_presets);
        let mut appearance = look;
        appearance.sanitize();
        self.user_presets.push(Preset {
            id: id.clone(),
            name: name.to_owned(),
            group: group.trim().to_owned(),
            appearance,
        });
        Some(id)
    }

    /// Only the user's own go; a built-in id deletes nothing.
    pub fn delete_preset(&mut self, id: &str) {
        self.user_presets.retain(|p| p.id != id);
    }

    /// Sets the field at dotted `path` to JSON `value`, then sanitizes.
    pub fn set_path(&mut self, path: &str, value: &str) -> Result<(), String> {
        let mut tree = serde_json::to_value(&*self).map_err(|e| e.to_string())?;
        let pointer = format!("/{}", path.replace('.', "/"));
        let slot = tree
            .pointer_mut(&pointer)
            .ok_or(format!("no setting {path}"))?;
        *slot = serde_json::from_str(value).map_err(|e| format!("{path}: {e}"))?;
        *self = serde_json::from_value(tree).map_err(|e| format!("{path}: {e}"))?;
        self.sanitize();
        Ok(())
    }

    /// Built-ins first, then the user's own: the menu's order.
    pub fn all_presets(&self) -> Vec<Preset> {
        let mut all = builtin_presets();
        all.extend(self.user_presets.iter().cloned());
        all
    }
}

/// `f32::clamp` that also turns NaN and infinities into `fallback`.
pub(crate) fn clamp(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_sets_one_field_and_rejects_unknown_ones() {
        let mut s = Settings::default();
        s.set_path("layout.page_indicator", "false").unwrap();
        assert!(!s.layout.page_indicator);
        s.set_path("text.font_size", "999").unwrap();
        assert!(s.text.font_size < 999.0, "sanitized");
        assert!(s.set_path("layout.nothing_here", "true").is_err());
        assert!(s.set_path("layout.page_indicator", "\"yes\"").is_err());
        assert!(!s.layout.page_indicator, "a failed set changes nothing");
    }

    #[test]
    fn a_round_trip_keeps_every_field() {
        let mut s = Settings::default();
        s.chrome.sidebar_overlay = true;
        s.motion.zoom = false;
        s.text.font_size = 21.0;
        s.text.serif_font = FontChoice::System(SystemFont::Georgia);
        s.workspace.look.outline_color = Swatch::Custom;
        s.dictionary.langs = vec!["en-my".into()];
        s.library.layout = mareader_library::Layout::List;
        s.library.columns = Some(4);
        s.read_aloud.rate = 1.5;
        s.read_aloud.skip_notes = true;
        s.library.set_sort(mareader_library::SortKey::Author);
        let back = load(&to_json(&s)).settings;
        assert_eq!(back, s);
    }

    #[test]
    fn a_library_view_out_of_range_or_missing_loads_sane() {
        let s = load(r#"{"version":2,"library":{"columns":99,"sort":"title"}}"#).settings;
        assert_eq!(s.library.columns, Some(mareader_library::COLUMNS_MAX));
        assert_eq!(s.library.sort, mareader_library::SortKey::Title);
        let old = load(r#"{"version":2}"#).settings;
        assert_eq!(old.library, mareader_library::View::default());
    }

    #[test]
    fn presets_save_trimmed_and_delete_by_id() {
        let mut s = Settings::default();
        let look = Appearance {
            tint_strength: 40,
            ..Appearance::default()
        };
        assert_eq!(s.save_preset("  ", "Night", look), None);
        let id = s.save_preset(" Night owl ", " Evening ", look);
        assert_eq!(id.as_deref(), Some("night-owl"));
        let again = s.save_preset("Night owl", "", look);
        assert_eq!(again.as_deref(), Some("night-owl-2"));
        let saved = s.user_presets.first();
        let fields = saved.map(|p| (p.name.as_str(), p.group.as_str(), p.appearance));
        assert_eq!(fields, Some(("Night owl", "Evening", look)));
        s.delete_preset("sepia");
        s.delete_preset("night-owl");
        let ids: Vec<_> = s.user_presets.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["night-owl-2"]);
        assert_eq!(s.all_presets().len(), builtin_presets().len() + 1);
    }

    #[test]
    fn sanitize_clamps_and_drops_bad_presets() {
        let mut s = Settings::default();
        s.layout.page_margin = f32::NAN;
        s.workspace.look.gap = 99.0;
        s.read_aloud.rate = f32::NAN;
        s.read_aloud.pitch = 9.0;
        s.read_aloud.engine = " ".into();
        s.read_aloud.speech_memory_mb = 0;
        s.read_aloud.model_idle_min = 7;
        let knobs = [("expression", 0.5), ("rhythm", f32::NAN), ("wild", 99.0)];
        let knobs = knobs.map(|(k, v)| (k.to_owned(), v)).into();
        s.read_aloud.knobs.insert("piper".into(), knobs);
        s.read_aloud
            .knobs
            .insert("gone".into(), [("x".into(), -1.0)].into());
        let mut shadow = builtin_presets().remove(0);
        shadow.name = "Mine".into();
        s.user_presets = vec![shadow];
        s.sanitize();
        assert_eq!(s.layout.page_margin, 0.0);
        assert_eq!(s.workspace.look.gap, PaneLook::MAX_GAP);
        assert_eq!(s.read_aloud.rate, 1.0);
        assert_eq!(s.read_aloud.pitch, 2.0);
        assert_eq!(s.read_aloud.engine, "system");
        assert_eq!(s.read_aloud.speech_memory_mb, 16);
        assert_eq!(s.read_aloud.model_idle_min, 2, "only offered times");
        let kept = [("piper".into(), [("expression".into(), 0.5)].into())].into();
        assert_eq!(s.read_aloud.knobs, kept, "only numbers in range stay");
        s.read_aloud.engine = "piper".into();
        assert_eq!(s.read_aloud.knobs().get("expression"), Some(&0.5));
        s.read_aloud.model_idle_min = 0;
        assert_eq!(s.read_aloud.model_idle(), None);
        s.read_aloud.model_idle_min = 5;
        assert_eq!(s.read_aloud.model_idle().map(|d| d.as_secs()), Some(300));
        assert!(s.user_presets.is_empty(), "a built-in id can't be shadowed");
    }
}
