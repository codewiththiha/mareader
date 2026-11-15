//! Semantic Lucide icons with optional generated animation.

pub(crate) mod anim;
/// Shared cache and pack-size identity for generated glyphs.
/// The UI-owned semantic `Icon` to generated pack-id adapter.
mod pack;
/// Generated one-frame resting glyphs.
mod rest;

use egui::{Color32, Image, Rect, Ui};

/// Lucide's grid: a glyph is drawn inside this square.
pub const GRID: f32 = mareader_animated_icons::GRID;
/// Authored animation headroom beyond each grid edge.
pub const PAD: f32 = mareader_animated_icons::PAD;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Sidebar,
    SidebarClose,
    Thumbnails,
    Outline,
    Dictionary,
    Library,
    Layout,
    Motion,
    Type,
    Levels,
    Clock,
    Search,
    Appearance,
    Settings,
    Pin,
    SplitRight,
    Close,
    WindowMinimize,
    WindowMaximize,
    WindowRestore,
    Book,
    Open,
    Light,
    Dark,
    Dim,
    Black,
    Menu,
    Fullscreen,
    ZoomIn,
    ZoomOut,
    FitWidth,
    FitPage,
    SinglePage,
    Spread,
    ScrollVertical,
    ScrollHorizontal,
    ExitFullscreen,
    Chromeless,
    Keyboard,
    Alert,
    Done,
    ChevronDown,
    ChevronUp,
    ChevronLeft,
    ChevronRight,
    Minus,
    Plus,
    Grid,
    List,
    Sort,
    ViewOptions,
    Folders,
    Folder,
    FolderPlus,
    Rescan,
    SearchFolder,
    ChooseFile,
    Offline,
    Remove,
    Shelf,
    Rename,
    Check,
    Import,
    Link,
    Restore,
    More,
    Copy,
    /// Anything the AI does: "AI" drawn as an icon, not a Lucide glyph.
    Ai,
    /// The AI's models: a processor chip, the AI mark's pair.
    Chip,
    /// A custom server's badge: it has no logo of its own.
    Server,
    /// Providers' own logos, from `assets/logos`.
    OpenAi,
    Gemini,
    OpenRouter,
    Ollama,
    LmStudio,
    Apple,
    /// Try a model once.
    Try,
    /// Say a word aloud.
    Speak,
    /// The full highlighter.
    Highlight,
    /// The half highlighter: a letter on a bar.
    HalfHighlight,
    Underline,
    MoveUp,
    MoveDown,
    /// An API key.
    Key,
    /// Sets a key aside.
    Ban,
    Swap,
    Download,
    Pause,
    Resume,
    LookUp,
    SelectSeveral,
    AutoScroll,
    /// A highlight's note, and the panel that gathers them.
    Note,
    Bookmark,
    /// The page AI's tasks.
    Summarize,
    Translate,
    Simplify,
    Stop,
    Show,
    /// Continuous mode: on as one reads.
    Repeat,
    /// Read-aloud: its bar, steps and the way out of a table.
    ReadAloud,
    /// A downloaded voice model's own settings tab.
    VoiceModel,
    /// The voices themselves, one mark per engine: `Icon::engine` picks them.
    Kokoro,
    Piper,
    Kitten,
    Phonon,
    SystemVoice,
    SilentEngine,
    SkipBack,
    SkipForward,
    /// Read-aloud's options: page numbers skipped, pages turned.
    PageNumber,
    Follow,
    LeaveTable,
    Table,
    Figure,
    /// A sidebar or boxed aside beside the main text.
    Aside,
}

impl Icon {
    /// Resolves an engine id to its mark.
    pub fn engine(id: &str) -> Option<Icon> {
        match id {
            "kokoro" => Some(Self::Kokoro),
            "piper" => Some(Self::Piper),
            "kitten" => Some(Self::Kitten),
            "phonon" => Some(Self::Phonon),
            mareader_speech::SYSTEM => Some(Self::SystemVoice),
            mareader_speech::SILENT => Some(Self::SilentEngine),
            _ => None,
        }
    }

    /// A custom mark: its loader URI and white SVG file.
    fn svg(self) -> Option<(&'static str, &'static str)> {
        let logo = |name, file| Some((name, file));
        match self {
            Self::OpenAi => logo(
                "bytes://mareader/logo-openai.svg",
                include_str!("../../assets/logos/openai.svg"),
            ),
            Self::Gemini => logo(
                "bytes://mareader/logo-gemini.svg",
                include_str!("../../assets/logos/gemini.svg"),
            ),
            Self::OpenRouter => logo(
                "bytes://mareader/logo-openrouter.svg",
                include_str!("../../assets/logos/openrouter.svg"),
            ),
            Self::Ollama => logo(
                "bytes://mareader/logo-ollama.svg",
                include_str!("../../assets/logos/ollama.svg"),
            ),
            Self::LmStudio => logo(
                "bytes://mareader/logo-lm-studio.svg",
                include_str!("../../assets/logos/lm-studio.svg"),
            ),
            Self::Apple => logo(
                "bytes://mareader/logo-apple.svg",
                include_str!("../../assets/logos/apple.svg"),
            ),
            _ => None,
        }
    }

    /// For egui widgets; tint it, or let a `Button` tint it.
    pub fn image(self, ui: &Ui, size: f32) -> Image<'static> {
        if let Some((uri, file)) = self.svg() {
            let source = egui::ImageSource::Bytes {
                uri: uri.into(),
                bytes: egui::load::Bytes::Static(file.as_bytes()),
            };
            return Image::new(source)
                .fit_to_exact_size(egui::Vec2::splat(size))
                .show_loading_spinner(false);
        }
        let Some(pack) = pack::of(self) else {
            return rest::blank(ui.ctx(), size);
        };
        rest::image(ui, pack, size)
    }

    /// For custom-painted widgets.
    pub fn paint(self, ui: &Ui, rect: Rect, color: Color32) {
        self.image(ui, rect.width()).tint(color).paint_at(ui, rect);
    }

    /// Stable glyph identity for transient animation keys.
    fn slot(self) -> u32 {
        self as u32
    }
}

/// One pack at one size, as the texture stores key it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct PackKey {
    pack: usize,
    pub(super) side: u16,
}

impl PackKey {
    pub(super) fn new(pack: &'static mareader_animated_icons::Pack, side: u16) -> Self {
        Self {
            pack: std::ptr::from_ref(pack) as usize,
            side,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_voice_engine_has_its_own_mark() {
        let ids: Vec<&str> = mareader_speech::MODELS
            .iter()
            .map(|m| m.id)
            .chain([mareader_speech::SYSTEM, mareader_speech::SILENT])
            .collect();
        let mut packs: Vec<&str> = Vec::new();
        for id in ids {
            let icon = Icon::engine(id).unwrap_or_else(|| panic!("{id}: no icon"));
            assert!(icon.svg().is_none(), "{id}: static SVG fallback");
            let pack = pack::of(icon).unwrap_or_else(|| panic!("{id}: no pack"));
            assert!(!packs.contains(&pack.id()), "{id}: a shared mark");
            packs.push(pack.id());
        }
        assert_eq!(packs.len(), 6, "six engines, six marks");
    }

    /// An engine this build does not know keeps the stock waveform, not a
    /// stranger's face.
    #[test]
    fn an_engine_without_a_mark_of_its_own_has_none() {
        assert!(Icon::engine("some-other-voice").is_none());
    }
}
