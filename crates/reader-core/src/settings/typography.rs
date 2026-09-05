//! The persisted typography of the reflowable formats.
//!
//! This is the SCHEMA half of the text formats' typography, and it sits with
//! the rest of the persisted settings for one reason: the field names below ARE
//! the storage contract, so they are additive only — every field carries
//! `#[serde(default)]` semantics through the struct-level default, and a blob
//! saved before a field existed loads with that field's default.
//!
//! The RESOLUTION half (choices becoming CSS font stacks, and the whole
//! setting becoming the custom properties the stylesheet reads) lives in
//! `reflow_core::typography`, which re-exports everything here so a caller can
//! import the type and its maths from one place. Fonts are chosen from the
//! system's faces today; [`BuiltInFont`] / [`builtin_fonts`] are the
//! deliberately-empty extension point a future release fills with faces shipped
//! inside the app — the schema already serialises them (`builtin:<name>`), so
//! adding one is adding a row to the table, not a migration.

use serde::{Deserialize, Serialize};

/// to exactly one of them, and each family has its own override slot in the
/// settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFamily {
    Serif,
    SansSerif,
    Monospace,
}

impl TextFamily {
    /// The generic CSS tail every stack of this family ends in.
    pub fn generic(self) -> &'static str {
        match self {
            Self::Serif => "serif",
            Self::SansSerif => "sans-serif",
            Self::Monospace => "monospace",
        }
    }
}

/// A font face that ships INSIDE the application, rather than one the OS
/// provides. The table is empty today — this type is the seam a future
/// release bolts bundled fonts onto: add a row, and every font picker and
/// every saved `builtin:<name>` choice resolves it. Nothing else moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltInFont {
    /// Stable identity — the `builtin:<name>` persisted in settings.
    pub id: &'static str,
    /// What the pickers show.
    pub label: &'static str,
    /// The CSS stack the face renders with (the bundled @font-face first,
    /// then fallbacks).
    pub stack: &'static str,
    /// Which family the face belongs to (picker grouping + fallbacks).
    pub family: TextFamily,
}

/// Every bundled font the reader knows. Empty until fonts ship with the
/// app; see [`BuiltInFont`].
pub fn builtin_fonts() -> &'static [BuiltInFont] {
    &[]
}

/// A system font face: one of the widely available cross-platform faces,
/// plus the four generic stacks. The id is what settings persist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemFont {
    // Generic stacks — "whatever the platform says", always available.
    UiSerif,
    UiSans,
    UiMono,
    // Serif faces.
    Georgia,
    TimesNewRoman,
    Palatino,
    Garamond,
    Baskerville,
    Charter,
    // Sans faces.
    Arial,
    Helvetica,
    Verdana,
    TrebuchetMs,
    Tahoma,
    GillSans,
    // Monospace faces.
    CourierNew,
    Menlo,
    Consolas,
    Monaco,
}

impl SystemFont {
    /// Every system face the pickers offer, in display order.
    pub fn all() -> &'static [SystemFont] {
        &[
            Self::UiSerif,
            Self::UiSans,
            Self::UiMono,
            Self::Georgia,
            Self::TimesNewRoman,
            Self::Palatino,
            Self::Garamond,
            Self::Baskerville,
            Self::Charter,
            Self::Arial,
            Self::Helvetica,
            Self::Verdana,
            Self::TrebuchetMs,
            Self::Tahoma,
            Self::GillSans,
            Self::CourierNew,
            Self::Menlo,
            Self::Consolas,
            Self::Monaco,
        ]
    }

    /// Stable identity — the `system:<id>` persisted in settings.
    pub fn id(self) -> &'static str {
        match self {
            Self::UiSerif => "ui-serif",
            Self::UiSans => "ui-sans",
            Self::UiMono => "ui-mono",
            Self::Georgia => "georgia",
            Self::TimesNewRoman => "times-new-roman",
            Self::Palatino => "palatino",
            Self::Garamond => "garamond",
            Self::Baskerville => "baskerville",
            Self::Charter => "charter",
            Self::Arial => "arial",
            Self::Helvetica => "helvetica",
            Self::Verdana => "verdana",
            Self::TrebuchetMs => "trebuchet-ms",
            Self::Tahoma => "tahoma",
            Self::GillSans => "gill-sans",
            Self::CourierNew => "courier-new",
            Self::Menlo => "menlo",
            Self::Consolas => "consolas",
            Self::Monaco => "monaco",
        }
    }

    /// Find a face by its persisted id.
    fn from_id(id: &str) -> Option<SystemFont> {
        Self::all().iter().copied().find(|f| f.id() == id)
    }

    /// What the pickers show.
    pub fn label(self) -> &'static str {
        match self {
            Self::UiSerif => "System Serif",
            Self::UiSans => "System Sans",
            Self::UiMono => "System Mono",
            Self::Georgia => "Georgia",
            Self::TimesNewRoman => "Times New Roman",
            Self::Palatino => "Palatino",
            Self::Garamond => "Garamond",
            Self::Baskerville => "Baskerville",
            Self::Charter => "Charter",
            Self::Arial => "Arial",
            Self::Helvetica => "Helvetica",
            Self::Verdana => "Verdana",
            Self::TrebuchetMs => "Trebuchet MS",
            Self::Tahoma => "Tahoma",
            Self::GillSans => "Gill Sans",
            Self::CourierNew => "Courier New",
            Self::Menlo => "Menlo",
            Self::Consolas => "Consolas",
            Self::Monaco => "Monaco",
        }
    }

    pub fn family(self) -> TextFamily {
        match self {
            Self::UiSerif | Self::Georgia | Self::TimesNewRoman | Self::Palatino
            | Self::Garamond | Self::Baskerville | Self::Charter => TextFamily::Serif,
            Self::UiMono | Self::CourierNew | Self::Menlo | Self::Consolas | Self::Monaco => {
                TextFamily::Monospace
            }
            _ => TextFamily::SansSerif,
        }
    }

    /// The CSS stack for this face: the face itself (quoted where a name
    /// carries spaces) followed by its family's generic tail.
    pub fn stack(self) -> String {
        let head = match self {
            Self::UiSerif => "ui-serif".to_string(),
            Self::UiSans => "ui-sans".to_string(),
            Self::UiMono => "ui-mono".to_string(),
            Self::Georgia => "Georgia".to_string(),
            Self::TimesNewRoman => "\"Times New Roman\"".to_string(),
            Self::Palatino => "Palatino, \"Palatino Linotype\"".to_string(),
            Self::Garamond => "Garamond, \"EB Garamond\"".to_string(),
            Self::Baskerville => "Baskerville, \"Baskerville Old Face\"".to_string(),
            Self::Charter => "Charter, \"Bitstream Charter\"".to_string(),
            Self::Arial => "Arial".to_string(),
            Self::Helvetica => "Helvetica, \"Helvetica Neue\"".to_string(),
            Self::Verdana => "Verdana".to_string(),
            Self::TrebuchetMs => "\"Trebuchet MS\"".to_string(),
            Self::Tahoma => "Tahoma".to_string(),
            Self::GillSans => "\"Gill Sans\", \"Gill Sans MT\"".to_string(),
            Self::CourierNew => "\"Courier New\"".to_string(),
            Self::Menlo => "Menlo".to_string(),
            Self::Consolas => "Consolas".to_string(),
            Self::Monaco => "Monaco".to_string(),
        };
        format!("{}, {}", head, self.family().generic())
    }

    /// Average glyph advance as a fraction of the font size. Feeds the
    /// pagination ESTIMATE (before the DOM measures real heights); a
    /// proportional face packs ~2 glyphs per em, a monospace face exactly
    /// 0.6em per cell.
    pub fn avg_char_width(self) -> f64 {
        match self.family() {
            TextFamily::Monospace => 0.6,
            TextFamily::Serif => 0.5,
            TextFamily::SansSerif => 0.52,
        }
    }
}

/// One font choice, as persisted and as the pickers express it.
///
/// The string encoding is the storage contract:
/// * `default` — resolve through the surrounding context (the reading font
///   for the body slot, the family's own stack for a family slot);
/// * `system:<id>` — a [`SystemFont`];
/// * `builtin:<id>` — a [`BuiltInFont`] (future: fonts shipped in the app).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FontChoice {
    /// Follow the context: the reader's default font in the body slot, or
    /// the family's natural stack in a family slot.
    #[default]
    Default,
    System(SystemFont),
    BuiltIn(String),
}

impl FontChoice {
    /// Parse the persisted form. Unknown ids fall back to [`FontChoice::Default`]
    /// rather than failing the whole settings blob.
    fn from_storage(s: &str) -> FontChoice {
        match s {
            "default" | "" => FontChoice::Default,
            rest => {
                if let Some(id) = rest.strip_prefix("system:") {
                    SystemFont::from_id(id)
                        .map(FontChoice::System)
                        .unwrap_or_default()
                } else if let Some(id) = rest.strip_prefix("builtin:") {
                    if id.is_empty() {
                        FontChoice::Default
                    } else {
                        FontChoice::BuiltIn(id.to_string())
                    }
                } else {
                    FontChoice::Default
                }
            }
        }
    }

    /// Produce the persisted form.
    fn to_storage(&self) -> String {
        match self {
            FontChoice::Default => "default".to_string(),
            FontChoice::System(f) => format!("system:{}", f.id()),
            FontChoice::BuiltIn(id) => format!("builtin:{id}"),
        }
    }
}

impl Serialize for FontChoice {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_storage())
    }
}

impl<'de> Deserialize<'de> for FontChoice {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(FontChoice::from_storage(&s))
    }
}

/// Default body size in CSS px at scale 1.
pub const DEFAULT_FONT_SIZE: f64 = 17.0;

/// The reader's idea of a neutral paragraph: 1em of space under it.
pub const DEFAULT_PARAGRAPH_MARGIN: f64 = 1.0;

/// Default line height (unitless multiple of the font size).
pub const DEFAULT_LINE_HEIGHT: f64 = 1.7;

/// Default body-ink intensity: the theme's full ink.
pub const DEFAULT_INK_CONTRAST: f64 = 100.0;

/// Where the reading column sits inside the viewport while a reflowable
/// document streams continuously (the vertical scroll mode). The text
/// itself keeps its natural alignment — this positions the COLUMN, exactly
/// the way a narrower book page sits left, centre or right on a desk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextColumnAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl TextColumnAlign {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Center => "Center",
            Self::Right => "Right",
        }
    }

    /// The stylesheet class that positions the stream's reading column
    /// (defined in `styles/text.css` beside the stream itself).
    pub fn container_class(&self) -> &'static str {
        match self {
            Self::Left => "tx-align-left",
            Self::Center => "tx-align-center",
            Self::Right => "tx-align-right",
        }
    }
}

/// The persisted typography of the reflowable formats.
///
/// Every knob is independent and additive; a blob missing any of them loads
/// the defaults above. The ranges are enforced by [`sanitize`], which the
/// app runs on load AND on every write path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextSettings {
    /// Render pages as an open book: a gutter margin faces the spine, and
    /// in the two-up modes the pair reads as facing pages. Off, every page
    /// carries symmetric margins.
    pub book_layout: bool,
    /// Space under a paragraph, in ems of the body font size.
    pub paragraph_margin: f64,
    /// Line height, as a unitless multiple of the font size.
    pub line_height: f64,
    /// Extra space between words, in CSS px (at scale 1). May be negative,
    /// to tighten.
    pub word_spacing: f64,
    /// Extra space between letters, in ems. May be negative, to tighten.
    pub letter_spacing: f64,
    /// First-line indent of paragraphs, in ems.
    pub text_indent: f64,
    /// Stretch each line to both margins.
    pub justify: bool,
    /// Let the shaper break words at line ends (needs a language-aware
    /// hyphenator; the reader marks its text as English).
    pub hyphenation: bool,
    /// Body font size in CSS px at scale 1.
    pub font_size: f64,
    /// Body font weight, 100..=900.
    pub font_weight: u16,
    /// The font body text renders in. `Default` resolves to the serif
    /// reading stack.
    pub default_font: FontChoice,
    /// Override for the serif family. `Default` keeps the family's natural
    /// stack.
    pub serif_font: FontChoice,
    /// Override for the sans family. `Default` keeps the family's natural
    /// stack.
    pub sans_font: FontChoice,
    /// Override for the monospace family (code). `Default` keeps the
    /// family's natural stack.
    pub mono_font: FontChoice,
    /// Where the reading column sits in the viewport while a text document
    /// streams continuously. The paginated modes ignore it — their pages
    /// centre themselves the way every fixed sheet does.
    pub column_align: TextColumnAlign,
    /// Body-ink intensity, 0–100. 100 is the theme's full ink; below that
    /// the ink mixes toward the paper colour. A comfort dial for long
    /// reading, not a tint — the paper stays whatever the theme says.
    pub ink_contrast: f64,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            book_layout: false,
            paragraph_margin: DEFAULT_PARAGRAPH_MARGIN,
            line_height: DEFAULT_LINE_HEIGHT,
            word_spacing: 0.0,
            letter_spacing: 0.0,
            text_indent: 0.0,
            justify: false,
            hyphenation: false,
            font_size: DEFAULT_FONT_SIZE,
            font_weight: 400,
            default_font: FontChoice::Default,
            serif_font: FontChoice::Default,
            sans_font: FontChoice::Default,
            mono_font: FontChoice::Default,
            column_align: TextColumnAlign::default(),
            ink_contrast: DEFAULT_INK_CONTRAST,
        }
    }
}

/// Clamp every field into its supported range. Runs on load and on every
/// write; idempotent.
pub fn sanitize(s: &mut TextSettings) {
    s.paragraph_margin = s.paragraph_margin.clamp(0.0, 3.0);
    s.line_height = s.line_height.clamp(1.0, 3.0);
    s.word_spacing = s.word_spacing.clamp(-2.0, 10.0);
    s.letter_spacing = s.letter_spacing.clamp(-0.02, 0.3);
    s.text_indent = s.text_indent.clamp(0.0, 4.0);
    s.font_size = s.font_size.clamp(10.0, 32.0);
    s.font_weight = s.font_weight.clamp(100, 900);
    s.ink_contrast = s.ink_contrast.clamp(0.0, 100.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_documented_middle() {
        let s = TextSettings::default();
        assert!(!s.book_layout);
        assert_eq!(s.paragraph_margin, 1.0);
        assert_eq!(s.line_height, 1.7);
        assert_eq!(s.word_spacing, 0.0);
        assert_eq!(s.letter_spacing, 0.0);
        assert_eq!(s.text_indent, 0.0);
        assert!(!s.justify);
        assert!(!s.hyphenation);
        assert_eq!(s.font_size, 17.0);
        assert_eq!(s.font_weight, 400);
        assert_eq!(s.default_font, FontChoice::Default);
        assert_eq!(s.column_align, TextColumnAlign::Center);
        assert_eq!(s.ink_contrast, 100.0);
    }

    #[test]
    fn an_empty_blob_loads_as_the_defaults() {
        let s: TextSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, TextSettings::default());
    }

    #[test]
    fn font_choices_round_trip_through_storage() {
        for choice in [
            FontChoice::Default,
            FontChoice::System(SystemFont::Georgia),
            FontChoice::System(SystemFont::TrebuchetMs),
            FontChoice::BuiltIn("reader-serif".into()),
        ] {
            let stored = choice.to_storage();
            assert_eq!(FontChoice::from_storage(&stored), choice, "{stored}");
        }
        // Unknown ids fall back to Default rather than failing the blob.
        assert_eq!(FontChoice::from_storage("system:nope"), FontChoice::Default);
        assert_eq!(FontChoice::from_storage("builtin:"), FontChoice::Default);
        assert_eq!(FontChoice::from_storage("junk"), FontChoice::Default);
    }

    #[test]
    fn settings_round_trip_with_fonts() {
        let mut s = TextSettings::default();
        s.default_font = FontChoice::System(SystemFont::Baskerville);
        s.mono_font = FontChoice::System(SystemFont::Consolas);
        s.serif_font = FontChoice::BuiltIn("future".into());
        let json = serde_json::to_string(&s).unwrap();
        let back: TextSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        assert!(json.contains("\"default_font\":\"system:baskerville\""), "{json}");
        assert!(json.contains("\"mono_font\":\"system:consolas\""), "{json}");
        assert!(json.contains("\"serif_font\":\"builtin:future\""), "{json}");
    }

    #[test]
    fn sanitize_clamps_everything_into_range() {
        let mut s = TextSettings {
            paragraph_margin: 9.0,
            line_height: 0.2,
            word_spacing: 99.0,
            letter_spacing: -5.0,
            text_indent: 40.0,
            font_size: 2.0,
            font_weight: 9999,
            ink_contrast: 500.0,
            ..TextSettings::default()
        };
        sanitize(&mut s);
        assert_eq!(s.paragraph_margin, 3.0);
        assert_eq!(s.line_height, 1.0);
        assert_eq!(s.word_spacing, 10.0);
        assert_eq!(s.letter_spacing, -0.02);
        assert_eq!(s.text_indent, 4.0);
        assert_eq!(s.font_size, 10.0);
        assert_eq!(s.font_weight, 900);
        assert_eq!(s.ink_contrast, 100.0);
    }

    #[test]
    fn column_align_carries_labels_and_classes() {
        assert_eq!(TextColumnAlign::default(), TextColumnAlign::Center);
        let all = [TextColumnAlign::Left, TextColumnAlign::Center, TextColumnAlign::Right];
        // Every choice names itself, and positions with its own class.
        for (i, a) in all.iter().enumerate() {
            assert_eq!(a.label(), ["Left", "Center", "Right"][i]);
            for b in &all[i + 1..] {
                assert_ne!(a.container_class(), b.container_class());
            }
        }
    }

    #[test]
    fn system_fonts_carry_consistent_metadata() {
        for f in SystemFont::all() {
            assert!(!f.id().is_empty());
            assert!(!f.label().is_empty());
            // Every stack ends in its family's generic keyword.
            assert!(
                f.stack().ends_with(f.family().generic()),
                "{:?}: {}",
                f,
                f.stack()
            );
            let w = f.avg_char_width();
            assert!(w > 0.4 && w < 0.7, "{w}");
        }
        // Ids are unique — the storage key depends on it.
        let all = SystemFont::all();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.id(), b.id());
            }
        }
    }
}
