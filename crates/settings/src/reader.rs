//! How the reader and its chrome behave: bars, layout, paper, motion.

use serde::{Deserialize, Serialize};

use crate::clamp;

/// How the Appearance popover arranges its controls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMenuStyle {
    /// Folded sections with horizontal sliders.
    #[default]
    Classic,
    /// Compact tabs with a 2D colour wheel and rotary dials.
    DialDeck,
}

impl AppearanceMenuStyle {
    pub const ALL: [Self; 2] = [Self::Classic, Self::DialDeck];

    pub fn label(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::DialDeck => "Dial deck",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChromeSettings {
    /// Each page's own title bar pin; read and set through `Bar`.
    pub titlebar_pinned: bool,
    pub library_titlebar_pinned: bool,
    pub dictionary_titlebar_pinned: bool,
    /// Retained for older settings; the sidebar now chooses its mode automatically.
    pub sidebar_overlay: bool,
    /// The bars blur the page behind them; off, they are solid.
    pub glass: bool,
    /// Which layout the Appearance popover opens in.
    pub appearance_menu: AppearanceMenuStyle,
}

impl Default for ChromeSettings {
    fn default() -> Self {
        Self {
            titlebar_pinned: false,
            library_titlebar_pinned: true,
            dictionary_titlebar_pinned: true,
            sidebar_overlay: false,
            glass: true,
            appearance_menu: AppearanceMenuStyle::Classic,
        }
    }
}

/// A page with a title bar of its own; each keeps its own pin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bar {
    Reader,
    Library,
    Dictionary,
}

impl Bar {
    pub const ALL: [Bar; 3] = [Bar::Reader, Bar::Library, Bar::Dictionary];
}

impl ChromeSettings {
    /// Whether `bar` stays shown rather than hiding until hovered.
    pub fn pinned(&self, bar: Bar) -> bool {
        match bar {
            Bar::Reader => self.titlebar_pinned,
            Bar::Library => self.library_titlebar_pinned,
            Bar::Dictionary => self.dictionary_titlebar_pinned,
        }
    }

    /// Pins or frees `bar` alone; the others keep theirs.
    pub fn pin(&mut self, bar: Bar, on: bool) {
        let slot = match bar {
            Bar::Reader => &mut self.titlebar_pinned,
            Bar::Library => &mut self.library_titlebar_pinned,
            Bar::Dictionary => &mut self.dictionary_titlebar_pinned,
        };
        *slot = on;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageIndicatorStyle {
    #[default]
    PageNumber,
    Percentage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatingLabelStyle {
    #[default]
    FileName,
    Chapter,
}

/// How a freshly opened document is zoomed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupFit {
    Page,
    #[default]
    Width,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutSettings {
    pub page_indicator: bool,
    pub page_indicator_style: PageIndicatorStyle,
    pub floating_label: bool,
    pub floating_label_style: FloatingLabelStyle,
    /// Keeps the label shown instead of fading it while idle.
    pub floating_label_persist: bool,
    /// Widest the label may grow, as a percent of the pane.
    pub floating_label_max_pct: f32,
    pub progress_bar: bool,
    pub startup_fit: StartupFit,
    /// Zoom used when `startup_fit` can't decide, e.g. no page yet.
    pub default_zoom: f32,
    pub no_gap: bool,
    /// A zoom set by hand keeps its scale on pages of another size.
    pub auto_resize: bool,
    pub page_shadow: bool,
    /// Extra margin around pages, in points.
    pub page_margin: f32,
}

impl LayoutSettings {
    pub const ZOOM: (f32, f32) = (0.25, 5.0);
    pub const MARGIN: (f32, f32) = (0.0, 64.0);
    pub const LABEL_PCT: (f32, f32) = (10.0, 100.0);

    pub fn sanitize(&mut self) {
        let d = Self::default();
        let (lo, hi) = Self::ZOOM;
        self.default_zoom = clamp(self.default_zoom, lo, hi, d.default_zoom);
        let (lo, hi) = Self::MARGIN;
        self.page_margin = clamp(self.page_margin, lo, hi, d.page_margin);
        let (lo, hi) = Self::LABEL_PCT;
        let max = clamp(
            self.floating_label_max_pct,
            lo,
            hi,
            d.floating_label_max_pct,
        );
        self.floating_label_max_pct = max;
    }
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            page_indicator: true,
            page_indicator_style: PageIndicatorStyle::PageNumber,
            floating_label: true,
            floating_label_style: FloatingLabelStyle::Chapter,
            floating_label_persist: true,
            floating_label_max_pct: 100.0,
            progress_bar: false,
            startup_fit: StartupFit::Width,
            default_zoom: 1.0,
            no_gap: false,
            auto_resize: true,
            page_shadow: false,
            page_margin: 0.0,
        }
    }
}

/// Where on a PDF page the backdrop reads its paper colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperArea {
    #[default]
    WholePage,
    Edges,
}

/// The backdrop takes the PDF page's own paper colour, themed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaperSettings {
    pub blend: bool,
    pub area: PaperArea,
}

/// `enabled` gates the rest; each switch keeps its value while off.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MotionSettings {
    pub enabled: bool,
    pub icon_animations: bool,
    pub sidebar_slide: bool,
    pub canvas_resize: bool,
    pub zoom: bool,
    pub scroll_jumps: bool,
}

impl MotionSettings {
    pub fn icon_animations(&self) -> bool {
        self.enabled && self.icon_animations
    }
    pub fn sidebar_slide(&self) -> bool {
        self.enabled && self.sidebar_slide
    }

    pub fn canvas_resize(&self) -> bool {
        self.enabled && self.canvas_resize
    }

    pub fn zoom(&self) -> bool {
        self.enabled && self.zoom
    }

    pub fn scroll_jumps(&self) -> bool {
        self.enabled && self.scroll_jumps
    }
}

impl Default for MotionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            icon_animations: true,
            sidebar_slide: true,
            canvas_resize: true,
            zoom: true,
            scroll_jumps: true,
        }
    }
}

/// How much of what is read aloud is lit at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Highlight {
    #[default]
    Sentence,
    /// The phrases a voice pauses between, as at commas.
    Phrases,
    /// [`ReadAloudSettings::highlight_words`] words at a time.
    Words,
}

/// What the player opens into when the words read are out of sight.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Player {
    /// Just the bar; it opens only when asked, and then compact.
    Bar,
    /// The bar with one line of words streaming above it.
    #[default]
    Compact,
    /// A card with the cover and the sentences round the one read.
    Full,
}

/// What reading does on reaching a part such as a table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Skip {
    /// Off, the part is read like the rest.
    pub on: bool,
    /// On: asks each time, else passes over it unasked.
    pub ask: bool,
}

impl Default for Skip {
    fn default() -> Self {
        Self {
            on: false,
            ask: true,
        }
    }
}

impl Skip {
    pub fn pass(self) -> Pass {
        match (self.on, self.ask) {
            (false, _) => Pass::Read,
            (true, true) => Pass::Ask,
            (true, false) => Pass::Skip,
        }
    }
}

/// What reading does at a part, as a [`Skip`] decides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pass {
    #[default]
    Read,
    Ask,
    Skip,
}

/// The parts reading may pass over, each with its own rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Skips {
    pub tables: Skip,
    pub lists: Skip,
    pub figures: Skip,
    /// Sidebars and boxed asides beside the main text.
    pub asides: Skip,
}

/// How a book with narration is read aloud.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadAloudSettings {
    /// Playback speed; 1 is the narrator's own pace.
    pub rate: f32,
    pub skip_notes: bool,
    pub skip_page_numbers: bool,
    /// Tables and the like: read, asked about, or passed over.
    pub skips: Skips,
    /// Per-library-book overrides: one bit for each kind of part.
    pub book_skips: std::collections::BTreeMap<mareader_library::BookId, u8>,
    /// Pages turn to keep the phrase read in view.
    pub follow: bool,
    pub highlight: Highlight,
    /// Words lit at a time when [`Highlight::Words`], 1 to 10.
    pub highlight_words: u8,
    pub player: Player,
    /// The player's width set by hand, in points; none is the narrowest.
    pub player_width: Option<f32>,
    /// The engine that reads a book with no narration of its own.
    pub engine: String,
    /// The voice picked for each engine, by engine; none is its default.
    pub voices: std::collections::BTreeMap<String, String>,
    /// The build each model runs, by model; none is its best one here.
    pub builds: std::collections::BTreeMap<String, String>,
    /// Each model's own settings by model, then by setting; 1 is normal.
    pub knobs: std::collections::BTreeMap<String, std::collections::BTreeMap<String, f32>>,
    /// Shares of the voice's own pitch and of full volume.
    pub pitch: f32,
    pub volume: f32,
    /// Megabytes of speech voice models keep made, to say again at once.
    pub speech_memory_mb: u32,
    /// Minutes an unused voice model stays loaded; 0 keeps it while reading.
    pub model_idle_min: u32,
}

impl ReadAloudSettings {
    pub const SLOWEST: f32 = 0.5;
    pub const FASTEST: f32 = 3.0;

    pub const LOWEST: f32 = 0.5;
    pub const HIGHEST: f32 = 2.0;
    /// The engine every system has.
    pub const SYSTEM: &str = "system";
    /// Least, default and most speech kept, in megabytes.
    pub const MEMORY_MB: (u32, u32, u32) = (16, 64, 1024);
    /// The idle times offered, in minutes; 0 is while reading.
    pub const IDLE_MIN: [u32; 5] = [1, 2, 5, 15, 0];
    /// A model's setting at most, as a share of its normal.
    pub const KNOB_MOST: f32 = 4.0;
    /// Words lit at a time at most.
    pub const WORDS_MOST: u8 = 10;
    /// The player's width set by hand, least and most, in points.
    pub const PLAYER_WIDTH: (f32, f32) = (300.0, 960.0);

    pub fn sanitize(&mut self) {
        let keep = |v: f32, lo: f32, hi: f32| match v.is_finite() {
            true => v.clamp(lo, hi),
            false => 1.0,
        };
        self.book_skips.retain(|_, mask| {
            *mask &= 0x0f;
            *mask != 0
        });
        while self.book_skips.len() > 1024 {
            self.book_skips.pop_last();
        }
        self.rate = keep(self.rate, Self::SLOWEST, Self::FASTEST);
        self.pitch = keep(self.pitch, Self::LOWEST, Self::HIGHEST);
        self.volume = keep(self.volume, 0.0, 1.0);
        self.highlight_words = self.highlight_words.clamp(1, Self::WORDS_MOST);
        let (narrowest, widest) = Self::PLAYER_WIDTH;
        let width = self.player_width.filter(|w| w.is_finite());
        self.player_width = width.map(|w| w.clamp(narrowest, widest));
        if self.engine.trim().is_empty() {
            self.engine = Self::SYSTEM.to_owned();
        }
        let (least, _, most) = Self::MEMORY_MB;
        self.speech_memory_mb = self.speech_memory_mb.clamp(least, most);
        if !Self::IDLE_MIN.contains(&self.model_idle_min) {
            self.model_idle_min = Self::default().model_idle_min;
        }
        for knobs in self.knobs.values_mut() {
            knobs.retain(|_, v| v.is_finite() && (0.0..=Self::KNOB_MOST).contains(v));
        }
        self.knobs.retain(|_, knobs| !knobs.is_empty());
    }

    /// The engine in use's own settings; none set is empty.
    pub fn knobs(&self) -> std::collections::BTreeMap<String, f32> {
        self.knobs_for(&self.engine)
    }

    /// `engine`'s own settings; none set is empty.
    pub fn knobs_for(&self, engine: &str) -> std::collections::BTreeMap<String, f32> {
        self.knobs.get(engine).cloned().unwrap_or_default()
    }

    /// How long an unused model stays loaded; `None` while reading.
    pub fn model_idle(&self) -> Option<std::time::Duration> {
        let minutes = u64::from(self.model_idle_min);
        (minutes > 0).then(|| std::time::Duration::from_secs(minutes * 60))
    }

    /// The voice picked for the engine in use, if any.
    pub fn voice(&self) -> Option<&str> {
        self.voice_for(&self.engine)
    }

    /// The voice picked for `engine`, if any.
    pub fn voice_for(&self, engine: &str) -> Option<&str> {
        self.voices.get(engine).map(String::as_str)
    }

    /// The build picked for the engine in use, if any.
    pub fn build(&self) -> Option<&str> {
        self.build_for(&self.engine)
    }

    /// The build picked for `engine`, if any.
    pub fn build_for(&self, engine: &str) -> Option<&str> {
        self.builds.get(engine).map(String::as_str)
    }
}

impl Default for ReadAloudSettings {
    fn default() -> Self {
        Self {
            rate: 1.0,
            skip_notes: false,
            skip_page_numbers: true,
            skips: Skips::default(),
            book_skips: std::collections::BTreeMap::new(),
            follow: true,
            highlight: Highlight::Sentence,
            highlight_words: 1,
            player: Player::Compact,
            player_width: None,
            engine: Self::SYSTEM.to_owned(),
            voices: std::collections::BTreeMap::new(),
            builds: std::collections::BTreeMap::new(),
            knobs: std::collections::BTreeMap::new(),
            pitch: 1.0,
            volume: 1.0,
            speech_memory_mb: Self::MEMORY_MB.1,
            model_idle_min: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_animations_follow_the_master_switch() {
        let mut m = MotionSettings::default();
        assert!(m.icon_animations());
        m.enabled = false;
        assert!(!m.icon_animations());
        assert!(m.icon_animations, "the user's per-motion choice is kept");
    }

    #[test]
    fn each_bar_keeps_its_own_pin() {
        let mut c = ChromeSettings::default();
        let pins = |c: &ChromeSettings| Bar::ALL.map(|b| c.pinned(b));
        assert_eq!(pins(&c), [false, true, true], "the defaults");
        c.pin(Bar::Dictionary, false);
        assert_eq!(pins(&c), [false, true, false]);
        c.pin(Bar::Reader, true);
        assert_eq!(pins(&c), [true, true, false]);
    }

    #[test]
    fn a_pin_saved_before_the_dictionarys_keeps_its_value() {
        let old = r#"{"titlebar_pinned":true,"library_titlebar_pinned":false}"#;
        let c: ChromeSettings = serde_json::from_str(old).expect("old settings read");
        let pins = Bar::ALL.map(|b| c.pinned(b));
        assert_eq!(pins, [true, false, true], "the new pin takes its default");
        assert_eq!(c.appearance_menu, AppearanceMenuStyle::Classic);
    }

    #[test]
    fn appearance_menu_style_round_trips_from_snake_case() {
        let raw = r#"{"appearance_menu":"dial_deck"}"#;
        let c: ChromeSettings = serde_json::from_str(raw).expect("dial deck reads");
        assert_eq!(c.appearance_menu, AppearanceMenuStyle::DialDeck);
    }
}
