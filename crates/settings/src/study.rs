//! Reading aids: word glosses, CEFR highlighting and dictionaries.

use mareader_appearance::Rgb;
use serde::{Deserialize, Serialize};

pub use mareader_library::Pen;

use crate::clamp;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlossDensity {
    #[default]
    Compact,
    Comfortable,
}

impl GlossDensity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Comfortable => "Comfortable",
        }
    }
}

/// The word card's spacing; AI marks are drawn as `HighlightSettings::ai`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlossSettings {
    pub density: GlossDensity,
}

/// A button the selection menu can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarItem {
    Copy,
    Explain,
    LookUp,
    /// Highlights the text and opens a note on it.
    Note,
    /// Reads the selected text aloud, and only it.
    ReadAloud,
    Highlight(Pen),
    /// Opens the AI skills: Translate, Summarize, Simplify, the reader's.
    Skills,
}

impl BarItem {
    /// Every button, in the order a new menu lists them.
    pub const ALL: [BarItem; 9] = [
        BarItem::Copy,
        BarItem::Explain,
        BarItem::Skills,
        BarItem::LookUp,
        BarItem::ReadAloud,
        BarItem::Note,
        BarItem::Highlight(Pen::Full),
        BarItem::Highlight(Pen::Underline),
        BarItem::Highlight(Pen::Half),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::Explain => "AI Dictionary",
            Self::LookUp => "Look up",
            Self::Note => "Note",
            Self::ReadAloud => "Read aloud",
            Self::Highlight(pen) => pen.label(),
            Self::Skills => "Skills",
        }
    }
}

/// What a highlighter button does when pressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickColor {
    /// Highlights at once in the pen's colour, which later edits follow.
    #[default]
    Default,
    /// Shows a row of colours to pick from each time.
    Ask,
}

/// A look kept for one purpose, so no highlight can pass for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reserved {
    pub pen: Pen,
    pub color: Rgb,
}

/// The three highlighters, the selection menu and the reserved looks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HighlightSettings {
    pub full: Rgb,
    pub half: Rgb,
    pub underline: Rgb,
    /// The fills' strength; underlines are drawn solid.
    pub opacity: f32,
    /// Legacy menu preference retained for compatibility; the menu ignores it.
    pub bar: Vec<BarItem>,
    pub pick: PickColor,
    /// How words the AI explained are marked.
    pub ai: Reserved,
    /// How words above the reader's level are shaded.
    pub cefr: Reserved,
    /// How a word recalled right in Reactive study mode is marked.
    pub solved: Reserved,
    /// How a word missed in Reactive study mode is marked.
    pub missed: Reserved,
}

impl HighlightSettings {
    pub const OPACITY: (f32, f32) = (0.2, 0.8);
    /// The colours offered.
    pub const PALETTE: [Rgb; 8] = [
        Rgb(0xff, 0xff, 0x00),
        Rgb(0xff, 0xbe, 0x00),
        Rgb(0xff, 0x00, 0x00),
        Rgb(0x83, 0xd3, 0x3c),
        Rgb(0x00, 0xb2, 0xf4),
        Rgb(0x16, 0x77, 0xff),
        Rgb(0xa5, 0x8a, 0xf0),
        Rgb(0xff, 0x00, 0xff),
    ];

    /// `pen`'s own colour.
    pub fn color(&self, pen: Pen) -> Rgb {
        match pen {
            Pen::Full => self.full,
            Pen::Half => self.half,
            Pen::Underline => self.underline,
        }
    }

    pub fn color_mut(&mut self, pen: Pen) -> &mut Rgb {
        match pen {
            Pen::Full => &mut self.full,
            Pen::Half => &mut self.half,
            Pen::Underline => &mut self.underline,
        }
    }

    pub fn reserved(&self) -> [Reserved; 4] {
        [self.ai, self.cefr, self.solved, self.missed]
    }

    /// Whether the reader may highlight with `pen` in `color`.
    pub fn allowed(&self, pen: Pen, color: Rgb) -> bool {
        !self.reserved().contains(&Reserved { pen, color })
    }

    /// The colours `pen` may take: its own first, then the palette's.
    pub fn colors(&self, pen: Pen) -> Vec<Rgb> {
        let own = self.color(pen);
        let mut out = vec![own];
        out.extend(Self::PALETTE.into_iter().filter(|c| *c != own));
        out.retain(|c| self.allowed(pen, *c));
        out
    }

    pub fn sanitize(&mut self) {
        let (lo, hi) = Self::OPACITY;
        self.opacity = clamp(self.opacity, lo, hi, Self::default().opacity);
        let mut seen = Vec::with_capacity(self.bar.len());
        self.bar.retain(|item| {
            let new = !seen.contains(item);
            seen.push(*item);
            new
        });
        // Shading never passes for an AI mark: it takes another colour.
        if self.cefr == self.ai {
            let (ai, own) = (self.ai.color, self.color(self.cefr.pen));
            let free = Self::PALETTE.into_iter().find(|c| *c != ai && *c != own);
            self.cefr.color = free.unwrap_or(Self::default().cefr.color);
        }
        // Missed never passes for solved.
        if self.missed == self.solved {
            self.missed = Self::default().missed;
            self.solved = Self::default().solved;
        }
        // A pen whose colour became reserved moves to the next one free.
        for pen in Pen::ALL {
            if !self.allowed(pen, self.color(pen))
                && let Some(free) = self.colors(pen).first().copied()
            {
                *self.color_mut(pen) = free;
            }
        }
    }
}

impl Default for HighlightSettings {
    fn default() -> Self {
        Self {
            full: Rgb(0xff, 0xff, 0x00),
            half: Rgb(0x83, 0xd3, 0x3c),
            underline: Rgb(0x16, 0x77, 0xff),
            opacity: 0.5,
            bar: vec![
                BarItem::Copy,
                BarItem::Explain,
                BarItem::Skills,
                BarItem::LookUp,
                BarItem::ReadAloud,
                BarItem::Note,
                BarItem::Highlight(Pen::Full),
                BarItem::Highlight(Pen::Underline),
                BarItem::Highlight(Pen::Half),
            ],
            pick: PickColor::Default,
            ai: Reserved {
                pen: Pen::Full,
                color: Rgb(0xa5, 0x8a, 0xf0),
            },
            cefr: Reserved {
                pen: Pen::Full,
                color: Rgb(0xff, 0x00, 0x00),
            },
            solved: Reserved {
                pen: Pen::Underline,
                color: Rgb(0x2e, 0xa0, 0x4f),
            },
            missed: Reserved {
                pen: Pen::Underline,
                color: Rgb(0xe0, 0x3e, 0x3e),
            },
        }
    }
}

/// The reader's level; words above it get highlighted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CefrLevel {
    A2,
    B1,
    #[default]
    B2,
    C1,
    C2,
}

impl CefrLevel {
    pub const ALL: [CefrLevel; 5] = [Self::A2, Self::B1, Self::B2, Self::C1, Self::C2];

    /// The word-list band at this level; words above it light up.
    pub fn band(self) -> u8 {
        match self {
            Self::A2 => 2,
            Self::B1 => 3,
            Self::B2 => 4,
            Self::C1 => 5,
            Self::C2 => 6,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::A2 => "A2",
            Self::B1 => "B1",
            Self::B2 => "B2",
            Self::C1 => "C1",
            Self::C2 => "C2",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::A2 => "Highlights B1 and above",
            Self::B1 => "Highlights B2 and above",
            Self::B2 => "Highlights C1 and above",
            Self::C1 => "Highlights C2",
            Self::C2 => "Nothing left above C2",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CefrSettings {
    pub enabled: bool,
    pub level: CefrLevel,
    /// Clicking a highlighted word explains it.
    pub click_explain: bool,
}

impl Default for CefrSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            level: CefrLevel::B2,
            click_explain: true,
        }
    }
}

/// Which English a word's sound is given in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    #[default]
    Both,
    American,
    British,
}

impl Accent {
    pub const ALL: [Accent; 3] = [Self::Both, Self::American, Self::British];

    pub fn label(self) -> &'static str {
        match self {
            Self::Both => "Both",
            Self::American => "American",
            Self::British => "British",
        }
    }
}

/// How a word's sound is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Notation {
    /// As dictionaries write it: `/jɒt/`.
    #[default]
    Ipa,
    /// Spelled as it sounds, stress in bold: `yot`.
    Easy,
    /// The phonemes the voices read, stress on each vowel.
    Raw,
}

impl Notation {
    pub const ALL: [Notation; 3] = [Self::Ipa, Self::Easy, Self::Raw];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ipa => "IPA",
            Self::Easy => "Easy to read",
            Self::Raw => "Raw",
        }
    }

    /// "yacht", written this way.
    pub fn detail(self) -> &'static str {
        match self {
            Self::Ipa => "As dictionaries write it: “yacht” is /jɒt/",
            Self::Easy => "Spelled as it sounds: “yacht” is yot",
            Self::Raw => "Just as the voices read it: “yacht” is jˈɒt",
        }
    }
}

/// A word's sound on its cards, once the pronunciation is downloaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PronunciationSettings {
    pub shown: bool,
    /// The reading voice's English decides, over `accent`.
    pub follow_voice: bool,
    pub accent: Accent,
    pub notation: Notation,
}

impl Default for PronunciationSettings {
    fn default() -> Self {
        Self {
            shown: true,
            follow_voice: true,
            accent: Accent::Both,
            notation: Notation::Ipa,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudySettings {
    pub gloss: GlossSettings,
    pub highlights: HighlightSettings,
    pub cefr: CefrSettings,
    pub words: crate::words::WordListSettings,
    pub pronunciation: PronunciationSettings,
    pub mode: crate::StudyModeSettings,
}

impl StudySettings {
    pub fn sanitize(&mut self) {
        self.highlights.sanitize();
        self.words.sanitize();
        self.mode.sanitize();
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DictionarySettings {
    /// Looks a word up on hover rather than only on click.
    pub hover: bool,
    /// A word with no entry shows the closest ones on its card.
    pub near: bool,
    pub default_lang: Option<String>,
    /// The packs the Dictionary panel searches; none means every one.
    pub langs: Vec<String>,
    /// Translate between a fixed `from` and `to` language.
    pub pair: bool,
    pub from: Option<String>,
    pub to: Option<String>,
    /// The Dictionary page's last searches, newest first.
    pub recent: Vec<String>,
    /// How the reader's Dictionary button opens it.
    pub opens: DictionaryOpens,
    pub window: DictionaryWindow,
    /// Words in other languages are said by the engine's voices for them.
    pub other_voices: bool,
    pub ai_first: bool,
}

/// The reader's Dictionary: floating over the page, or the full page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DictionaryOpens {
    #[default]
    Window,
    Page,
}

/// The floating Dictionary: whether it shows, folded, and where.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DictionaryWindow {
    pub open: bool,
    /// Folded to a pill that names the last word.
    pub minimized: bool,
    /// Left, top, width and height; none sits it beside the page.
    pub rect: Option<[f32; 4]>,
}

/// The floating Dictionary's smallest size, in points.
pub const WINDOW_MIN: [f32; 2] = [300.0, 240.0];

impl DictionaryWindow {
    /// Drops a rect that isn't one; a small one grows to the minimum.
    pub fn sanitize(&mut self) {
        self.rect = self.rect.filter(|r| r.iter().all(|v| v.is_finite()));
        if let Some(r) = &mut self.rect {
            r[2] = r[2].max(WINDOW_MIN[0]);
            r[3] = r[3].max(WINDOW_MIN[1]);
        }
    }
}

/// How many searches the Dictionary page remembers.
pub const RECENT: usize = 12;

impl DictionarySettings {
    /// Keeps `word` first among the recent searches.
    pub fn remember(&mut self, word: &str) {
        let word = word.trim();
        if word.is_empty() {
            return;
        }
        self.recent.retain(|w| w != word);
        self.recent.insert(0, word.to_owned());
        self.recent.truncate(RECENT);
    }

    pub fn sanitize(&mut self) {
        let langs = [&mut self.default_lang, &mut self.from, &mut self.to];
        for lang in langs.into_iter().flatten() {
            *lang = mareader_dict::language_code(lang);
        }
        let recent = std::mem::take(&mut self.recent);
        recent.iter().rev().for_each(|w| self.remember(w));
        self.window.sanitize();
        let mut seen = Vec::with_capacity(self.langs.len());
        self.langs.retain(|id| {
            let new = !seen.contains(id);
            seen.push(id.clone());
            new
        });
    }
}

impl Default for DictionarySettings {
    fn default() -> Self {
        Self {
            hover: true,
            near: true,
            default_lang: None,
            langs: Vec::new(),
            pair: false,
            from: None,
            to: None,
            recent: Vec::new(),
            opens: DictionaryOpens::Window,
            window: DictionaryWindow::default(),
            other_voices: true,
            ai_first: false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn old_vocabulary_settings_ignore_removed_mark_field() {
        for raw in [
            "{\"enabled\":true}",
            "{\"enabled\":true,\"mark\":\"wash\"}",
            "{\"enabled\":true,\"mark\":\"dot\"}",
            "{\"enabled\":true,\"mark\":\"custom\"}",
        ] {
            let old: CefrSettings = serde_json::from_str(raw).unwrap();
            assert!(old.enabled);
            assert_eq!(old.level, CefrLevel::B2);
        }
    }

    #[test]
    fn a_window_rect_is_finite_and_never_below_the_minimum() {
        let mut w = DictionaryWindow {
            rect: Some([10.0, 20.0, 50.0, 900.0]),
            ..DictionaryWindow::default()
        };
        w.sanitize();
        assert_eq!(w.rect, Some([10.0, 20.0, WINDOW_MIN[0], 900.0]));
        w.rect = Some([f32::NAN, 0.0, 400.0, 400.0]);
        w.sanitize();
        assert_eq!(w.rect, None);
    }

    #[test]
    fn recent_searches_stay_unique_newest_first_and_bounded() {
        let mut d = DictionarySettings::default();
        for w in ["light", " dark ", "light", ""] {
            d.remember(w);
        }
        assert_eq!(d.recent, ["light", "dark"]);
        d.recent = (0..20).map(|i| format!("w{i}")).collect();
        d.recent.push("w1".into());
        d.to = Some("Russian".into());
        d.sanitize();
        assert_eq!(d.to.as_deref(), Some("ru"), "a name kept before tags");
        assert_eq!(d.recent.len(), RECENT);
        assert_eq!(d.recent.iter().filter(|w| *w == "w1").count(), 1);
    }

    #[test]
    fn the_ai_look_is_never_offered_to_its_pen() {
        let h = HighlightSettings::default();
        let violet = h.ai.color;
        assert!(!h.colors(Pen::Full).contains(&violet));
        assert!(!h.allowed(Pen::Full, violet));
        // Another pen may still use the colour: the pair is what is kept.
        assert!(h.colors(Pen::Underline).contains(&violet));
    }

    #[test]
    fn a_pen_on_a_newly_reserved_colour_moves_to_a_free_one() {
        let mut h = HighlightSettings::default();
        h.ai = Reserved {
            pen: Pen::Full,
            color: h.full,
        };
        h.sanitize();
        assert_ne!(h.full, h.ai.color);
        assert!(h.allowed(Pen::Full, h.full));
    }

    #[test]
    fn shading_keeps_its_look_apart_from_ai_marks_and_pens() {
        let mut h = HighlightSettings::default();
        assert!(!h.colors(Pen::Full).contains(&h.cefr.color));
        h.cefr = h.ai;
        h.full = h.ai.color;
        h.sanitize();
        assert_ne!(h.cefr, h.ai);
        assert!(h.allowed(Pen::Full, h.full));
        assert_ne!(h.full, h.cefr.color);
    }

    #[test]
    fn the_menu_keeps_each_button_once_and_bad_values_are_bounded() {
        let mut h = HighlightSettings {
            bar: vec![BarItem::Copy, BarItem::Highlight(Pen::Half), BarItem::Copy],
            opacity: f32::NAN,
            ..HighlightSettings::default()
        };
        h.sanitize();
        assert_eq!(h.bar, [BarItem::Copy, BarItem::Highlight(Pen::Half)]);
        assert!((h.opacity - 0.5).abs() < f32::EPSILON);
        let saved = serde_json::to_string(&h).unwrap();
        assert!(saved.contains(r#"{"highlight":"half"}"#), "{saved}");
        let back: HighlightSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!(back, h);
    }
}
