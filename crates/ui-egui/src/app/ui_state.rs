//! What the window alone owns.
//!
//! `MareaderApp` holds a [`Session`] and this. Everything here dies with the
//! window: controllers, animation clocks, textures, what a pointer is over.
//! What outlives a frame belongs to the session
//! (`docs/architecture.md` § Layers).

use super::*;

/// The front end's own state, beside the session's.
pub struct UiState {
    pub(super) platform: Platform,
    pub(super) titlebar: TitleBar,
    /// Distraction-free: bars stay hidden while reading.
    pub(super) chromeless: bool,
    /// Shots: the newest mark's menu opens once it is drawn.
    pub(super) menu_shot: bool,
    /// Shots: reads the focused book aloud once it opens.
    pub(super) read_aloud_shot: bool,
    /// Cards' transcripts, and their tracks, to open for a screenshot.
    pub(super) transcript_shot: Vec<(usize, Option<usize>)>,
    /// Trigger targets to show or hide for a screenshot.
    pub(super) trigger_shot: Vec<(usize, bool)>,
    /// Selectors a screenshot clicks on scripted pages, in order.
    pub(super) script_shot: Vec<String>,
    pub(super) scripts: crate::scripts::Runtimes,
    /// Shots and tests: a page AI result kept, or a run started (true).
    pub(super) page_ai_seed: Option<(usize, mareader_library::AiTask, bool, usize)>,
    /// Shots: pages the word list window opens on, once shown.
    pub(super) word_window_seed: Option<Vec<usize>>,
    pub(super) sidebar: Sidebar,
    pub(super) settings_window: SettingsWindow,
    pub(super) find: FindBar,
    pub(super) toasts: Toasts,
    /// The selection bar's answers, marks and the word card.
    pub(super) study: StudyUi,
    /// Study mode overlays: the proactive gate and the recall popover.
    pub(super) study_mode: crate::study::mode::StudyModeUi,
    pub(super) page_ai: crate::page_ai::PageAiUi,
    pub(super) read_aloud: crate::read_aloud::ReadAloudUi,
    /// Who answers Explain, and the AI settings' background work.
    pub(super) ai: crate::ai::Ai,
    /// Language packs, the lookup card and the Dictionary panel.
    pub(super) dict: crate::dict::DictUi,
    /// A stand-in English voice, for shots and tests.
    pub(super) english: Option<mareader_engine::sounds::EnglishVoice>,
    /// A selection the options asked for, made once its page is laid out.
    pub(super) staged: std::collections::VecDeque<Staged>,
    pub(super) lights: NativeLights,
    /// Where AppKit last drew the lights, once the binary measured them.
    pub(super) light_spots: Option<LightSpots>,
    pub(super) documents: DocumentsUi,
    pub(super) files: Files,
    pub(super) library: LibraryUi,
    /// The sidebar panels' scroll-following, for the focused pane.
    pub(super) outline: OutlineState,
    pub(super) thumbs: ThumbState,
    pub(super) word_window: crate::word_list::window::WordWindow,
    pub(super) shelf_tree: PanelState,
    /// Each pane's page bar.
    pub(super) page_bars: std::collections::BTreeMap<PaneId, PageBar>,
    /// Last frame's pane rects, to aim a file drop.
    pub(super) pane_rects: Vec<(PaneId, egui::Rect)>,
    /// A pane held up to be moved, till the press ends.
    pub(super) lifting: Option<crate::lift::Lift>,
    /// The library page's rect while it is up, for the glass behind bars.
    pub(super) library_rect: Option<egui::Rect>,
    /// What an empty pane offers, as of a library revision.
    pub(super) reopen: (u64, Vec<(mareader_library::BookId, String)>),
    /// Live look transition blending tokens, pane fills, and shaders.
    pub(super) fade: LookFade,
    pub(super) styled: Option<(Appearance, MotionSettings, bool)>,
    /// The system asks for less motion; set off the UI thread at launch.
    pub(super) reduced_motion: Arc<AtomicBool>,
    /// Whether [`MareaderApp::resume`] has already queued the saved document.
    pub(super) resumed: bool,
    pub(super) fonts: FontState,
    /// Fills in letters no bundled face has from the machine's fonts.
    pub(super) machine_fonts: MachineFonts,
    /// Commands from outside a view: tests, OS drops, later the keymap.
    pub(super) queued: Commands,
    /// Session events requiring an egui context, deferred from `drain_local`.
    pub(super) deferred_events: Vec<mareader_engine::event::Event>,
    /// Optional loopback live UI control channel host.
    pub(super) control: Option<crate::control::ControlHost>,
    pub(super) wake: setup::WakeSlot,
    #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
    pub(super) live: theme::live::LiveTokens,
}
