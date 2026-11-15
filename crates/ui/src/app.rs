//! The app state and its eframe loop.

mod access;
mod appearance;
mod apply;
mod frame;
mod glosses;
mod library;
mod navigation;
mod options;
mod page_ai;
mod panes;
mod read_aloud;
mod setup;
mod study;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use eframe::Storage;
use egui::{Context, Rect, ViewportCommand};
use mareader_appearance::{
    Appearance, BaseMode, FilmGrain, NoiseMode, PaneLooks, Rgb, Routing, TextureMode,
    builtin_presets,
};
use mareader_session::Anchor;
use mareader_settings::{
    AppearanceMenuStyle, Bar, BookDesign, ColumnAlign, DictionaryOpens, FontChoice, LibraryClick,
    MotionSettings, PaperArea, Settings, TextSettings,
};
use mareader_workspace::{
    Bounds, Edge, LiftTarget, MAX_PANES, PaneId, PaneTree, Side, SplitAxis, TreeError,
};

use crate::backdrop::Backdrop;
use crate::bookmarks;
use crate::command::{Command, Commands, Independence, WindowCommand};
use crate::crossfade::Crossfade;
use crate::documents::{Documents, OpenRequest, Placement};
use crate::files::Files;
use crate::find_bar::FindBar;
use crate::floating_label::{self, LabelChrome};
use crate::gpu::{Glass, Gpu, GrainCallback, Quad, crawl_offset};
use crate::layout::ViewMode;
use crate::library::{self as ui_library, LibraryState};
use crate::library_panel::{self, PaneTab, PanelContext, PanelState};
use crate::machine_fonts::MachineFonts;
use crate::notes_panel;
use crate::outline_panel::{self, OutlineState};
use crate::page_bar::{self, BarLook, PageBar};
use crate::pane_texts::PaneTexts;
use crate::panes::{self as ui_panes, PaneStyle, PaneView};
use crate::pdf_view::{PageLook, ViewContext};
use crate::platform::{LightSpots, NativeLights, Platform};
use crate::read_aloud::ReadFrom;
use crate::route::Route;
use crate::search::{Found, Lit, Search};
use crate::settings_window::{Scope, SettingsWindow, Shown, Tab};
use crate::sidebar::{Panel, RailContext, Sidebar};
use crate::study::{self as ui_study, Explained, Gloss, Highlighted, Study};
use crate::thumbnails::ThumbState;
use crate::titlebar::{self, BarContext, BarPage, DictionaryBar, LibraryBar, ReaderBar, TitleBar};
use crate::toast::{Toast, Toasts};
use crate::{keymap, motion, resize, theme};

pub const SETTINGS_KEY: &str = "settings";
/// Kept apart from settings: it says what was read, not how to read.
pub const LAST_DOCUMENT_KEY: &str = "last_document";

/// What reopens at launch: the file read at exit, on its page.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LastDocument {
    pub path: PathBuf,
    pub page: usize,
    /// A book's block and offset, which outlast a change of type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<(usize, usize)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spine: Option<mareader_library::SpinePosition>,
    #[serde(default)]
    pub pages: usize,
    /// The library book it was read as, so a twin reopens as itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub book: Option<mareader_library::BookId>,
}

/// Settings reach disk this long after a change, at most.
pub const SAVE_EVERY: Duration = Duration::from_millis(500);

/// A generated sample for lookup screenshots, not a provider response.
struct AssistantSample;

impl mareader_study::Explainer for AssistantSample {
    fn by(&self) -> String {
        "Assistant-generated sample".into()
    }

    fn explain(
        &self,
        word: &str,
        _context: &str,
        partial: &(dyn Fn(mareader_study::WordInfo) + Sync),
    ) -> Result<mareader_study::WordInfo, mareader_study::Failure> {
        if !word.eq_ignore_ascii_case("acknowledged") {
            return Err(mareader_study::Why::BadAnswer.into());
        }
        let info = mareader_study::WordInfo {
            pos: "verb".into(),
            meaning: concat!(
                "Accepted as true or recognized. Here, Austen frames a belief about ",
                "wealthy bachelors and marriage as unquestioned, with a wry edge."
            )
            .into(),
            synonyms: vec![
                "recognized".into(),
                "accepted".into(),
                "assumed".into(),
                "understood".into(),
                "taken for granted".into(),
            ],
            usages: vec![
                "Her contribution was acknowledged in the report.".into(),
                "He acknowledged that the first plan had failed.".into(),
                "The report acknowledges the limits of the evidence.".into(),
            ],
        };
        partial(mareader_study::WordInfo {
            pos: info.pos.clone(),
            meaning: info.meaning.clone(),
            ..Default::default()
        });
        Ok(info.clamped())
    }
}

pub struct MareaderApp {
    settings: Settings,
    platform: Platform,
    titlebar: TitleBar,
    /// Distraction-free: bars stay hidden while reading.
    chromeless: bool,
    /// Shots: the newest mark's menu opens once it is drawn.
    menu_shot: bool,
    /// Shots: reads the focused book aloud once it opens.
    read_aloud_shot: bool,
    /// Cards' transcripts, and their tracks, to open for a screenshot.
    transcript_shot: Vec<(usize, Option<usize>)>,
    /// Trigger targets to show or hide for a screenshot.
    trigger_shot: Vec<(usize, bool)>,
    /// Selectors a screenshot clicks on scripted pages, in order.
    script_shot: Vec<String>,
    scripts: crate::scripts::Runtimes,
    /// Clips the books' triggers play.
    clips: crate::trigger::Clips,
    /// Shots and tests: a page AI result kept, or a run started (true).
    page_ai_seed: Option<(usize, mareader_library::AiTask, bool, usize)>,
    /// Shots: pages the word list window opens on, once shown.
    word_window_seed: Option<Vec<usize>>,
    tree: PaneTree,
    active: PaneId,
    /// Panes by last focus, latest first: blend's paper follows them.
    recent: Vec<PaneId>,
    sidebar: Sidebar,
    settings_window: SettingsWindow,
    find: FindBar,
    search: Search,
    toasts: Toasts,
    /// The selection bar's answers, marks and the word card.
    study: Study,
    page_ai: crate::page_ai::PageAi,
    read_aloud: crate::read_aloud::ReadAloud,
    /// Who answers Explain, and the AI settings' background work.
    ai: crate::ai::Ai,
    /// Language packs, the lookup card and the Dictionary panel.
    dict: crate::dict::Dict,
    vocab: crate::vocab::Vocab,
    /// How card words sound; shares files with the voice models.
    sounds: crate::sounds::Sounds,
    /// A stand-in English voice, for shots and tests.
    english: Option<crate::sounds::EnglishVoice>,
    /// A selection the options asked for, made once its page is laid out.
    staged: std::collections::VecDeque<Staged>,
    lights: NativeLights,
    /// Where AppKit last drew the lights, once the binary measured them.
    light_spots: Option<LightSpots>,
    documents: Documents,
    files: Files,
    library: LibraryState,
    /// On the Dictionary page: the route it was opened from.
    dict_from: Option<Route>,
    /// The sidebar panels' scroll-following, for the focused pane.
    outline: OutlineState,
    thumbs: ThumbState,
    /// Picked pages' text on its way to the clipboard.
    page_copy: crate::page_copy::PageCopy,
    word_lists: crate::word_list::WordLists,
    word_window: crate::word_list::window::WordWindow,
    shelf_tree: PanelState,
    /// Each pane's page bar.
    page_bars: std::collections::BTreeMap<PaneId, PageBar>,
    /// Last frame's pane rects, to aim a file drop.
    pane_rects: Vec<(PaneId, egui::Rect)>,
    /// A pane held up to be moved, till the press ends.
    lifting: Option<crate::lift::Lift>,
    /// The library page's rect while it is up, for the glass behind bars.
    library_rect: Option<egui::Rect>,
    /// What an empty pane offers, as of a library revision.
    reopen: (u64, Vec<(mareader_library::BookId, String)>),
    /// Split panes' own looks, and the routing they were last fitted to.
    looks: PaneLooks<PaneId>,
    routed: Routing,
    /// Split panes' own text, while text is independent.
    texts: PaneTexts,
    /// Picks new panes' hues and textures.
    random: u64,
    /// A theme change held for, or fading over, a snapshot.
    fade: Crossfade<Held>,
    styled: Option<(Appearance, MotionSettings, bool)>,
    /// The system asks for less motion; set off the UI thread at launch.
    reduced_motion: Arc<AtomicBool>,
    /// Read from storage; [`MareaderApp::resume`] opens it.
    last: Option<LastDocument>,
    fonts: FontState,
    /// Fills in letters no bundled face has from the machine's fonts.
    machine_fonts: MachineFonts,
    /// Commands from outside a view: tests, OS drops, later the keymap.
    queued: Commands,
    #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
    live: theme::live::LiveTokens,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FontState {
    Missing,
    Installed,
    Ready,
}

/// A selection from the options: `text` on `page`, maybe explained.
struct Staged {
    page: usize,
    text: String,
    explain: bool,
    look_up: bool,
    pen: Option<mareader_library::Pen>,
    /// A note's words, and whether it is saved or left as a draft.
    note: Option<(String, bool)>,
    /// Its page was asked for once; the view is on its way there.
    turned: bool,
    /// Bookmarks the page instead of selecting on it.
    bookmark: bool,
    /// Picks every mark and opens Style… instead.
    pick: bool,
}

impl Default for MareaderApp {
    fn default() -> Self {
        Self::with_settings(Settings::default())
    }
}

impl MareaderApp {
    pub fn with_settings(mut settings: Settings) -> Self {
        settings.sanitize();
        Self {
            titlebar: TitleBar::new(settings.chrome.pinned(Bar::Reader)),
            settings,
            platform: Platform::current(),
            chromeless: false,
            menu_shot: false,
            read_aloud_shot: false,
            transcript_shot: Vec::new(),
            trigger_shot: Vec::new(),
            script_shot: Vec::new(),
            scripts: Default::default(),
            clips: crate::trigger::Clips::default(),
            page_ai_seed: None,
            word_window_seed: None,
            tree: PaneTree::new(),
            active: PaneId(1),
            recent: Vec::new(),
            sidebar: Sidebar::default(),
            settings_window: SettingsWindow::default(),
            find: FindBar::default(),
            search: Search::default(),
            toasts: Toasts::default(),
            study: Study::default(),
            page_ai: crate::page_ai::PageAi::default(),
            read_aloud: crate::read_aloud::ReadAloud::default(),
            ai: crate::ai::Ai::default(),
            dict: crate::dict::Dict::default(),
            vocab: crate::vocab::Vocab::default(),
            sounds: crate::sounds::Sounds::default(),
            english: None,
            staged: Default::default(),
            lights: NativeLights::default(),
            light_spots: None,
            documents: Documents::default(),
            files: Files::default(),
            library: LibraryState::default(),
            dict_from: None,
            outline: OutlineState::default(),
            thumbs: ThumbState::default(),
            page_copy: Default::default(),
            word_lists: Default::default(),
            word_window: Default::default(),
            shelf_tree: PanelState::default(),
            page_bars: std::collections::BTreeMap::new(),
            pane_rects: Vec::new(),
            lifting: None,
            library_rect: None,
            reopen: (u64::MAX, Vec::new()),
            looks: PaneLooks::default(),
            routed: Routing::default(),
            texts: PaneTexts::default(),
            random: 0x9e37_79b9_7f4a_7c15,
            fade: Crossfade::default(),
            styled: None,
            reduced_motion: Arc::new(AtomicBool::new(false)),
            last: None,
            fonts: FontState::Missing,
            machine_fonts: MachineFonts::default(),
            queued: Commands::new(),
            #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
            live: theme::live::LiveTokens::from_env(),
        }
    }

    pub fn from_creation(cc: &eframe::CreationContext<'_>) -> Self {
        Self::from_storage(cc.storage)
    }

    /// Unreadable settings repair section by section; the notes go to stderr.
    pub fn from_storage(storage: Option<&dyn Storage>) -> Self {
        let mut app = Self::settings_from(storage);
        let last = storage.and_then(|s| s.get_string(LAST_DOCUMENT_KEY));
        app.last = last.and_then(|json| serde_json::from_str(&json).ok());
        app
    }

    fn settings_from(storage: Option<&dyn Storage>) -> Self {
        let Some(json) = storage.and_then(|s| s.get_string(SETTINGS_KEY)) else {
            return Self::default();
        };
        let loaded = mareader_settings::load(&json);
        for note in &loaded.notes {
            eprintln!("mareader: {note}");
        }
        let repaired = !loaded.notes.is_empty();
        let mut app = Self::with_settings(loaded.settings);
        if repaired {
            let note = "Some settings couldn't be read and were reset to defaults.";
            app.toasts.push(Toast::error(note));
        }
        app
    }
}

/// What a cross-fade holds back: the settings and the panes' looks.
#[derive(Clone)]
struct Held {
    settings: Settings,
    looks: PaneLooks<PaneId>,
    routed: Routing,
}
