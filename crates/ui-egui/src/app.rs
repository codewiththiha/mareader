//! The app state and its eframe loop.

mod access;
mod appearance;
mod apply;
mod control;
mod frame;
mod glosses;
mod library;
mod navigation;
mod page_ai;
mod panes;
mod read_aloud;
mod setup;
mod skills;
mod space;
mod study;
mod study_mode;
mod ui_state;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use egui::{Context, Rect};
use mareader_appearance::{Appearance, FilmGrain, Rgb, Routing};
use mareader_settings::{DictionaryOpens, MotionSettings, Settings, TextSettings};
use mareader_workspace::{Bounds, LiftTarget, MAX_PANES, PaneId};

use crate::backdrop::Backdrop;
use crate::bookmarks;
use crate::crossfade::{LookFade, LookSnap, PaneSnap};
use crate::documents::{DocumentsUi, DocumentsView};
use crate::files::{Files, Runner};
use crate::find_bar::FindBar;
use crate::floating_label::{self, LabelChrome};
use crate::gpu::{Glass, Gpu, GrainCallback, Quad, crawl_offset};
use crate::library::{self as ui_library, LibraryUi, LibraryView};
use crate::library_panel::{self, PaneTab, PanelContext, PanelState};
use crate::machine_fonts::MachineFonts;
use crate::notes_panel;
use crate::outline_panel::{self, OutlineState};
use crate::page_bar::{self, BarLook, PageBar};
use crate::panes::{self as ui_panes, PaneStyle, PaneView};
use crate::pdf_view::{PageLook, ViewContext};
use crate::platform::{LightSpots, NativeLights, Platform};
use crate::settings_window::SettingsWindow;
use crate::sidebar::{Panel, RailContext, Sidebar};
use crate::study::{Explained, Gloss, StudyUi};
use crate::thumbnails::ThumbState;
use crate::titlebar::{self, BarContext, BarPage, DictionaryBar, LibraryBar, ReaderBar, TitleBar};
use crate::toast::Toasts;
use crate::ui_command::Allows;
use crate::ui_command::{Commands, Sent};
use crate::{keymap, motion, resize, theme};
use mareader_engine::command::{Command, Independence};
use mareader_engine::documents::Placement;
use mareader_engine::host::{Fake, Machine, Shared};
use mareader_engine::pane::ViewMode;
use mareader_engine::read_aloud::ReadFrom;
use mareader_engine::route::Route;
use mareader_engine::save::{LastDocument, Loaded};
use mareader_engine::scope::{Scope, Shown};
use mareader_engine::search::{Found, Lit, Search};
use mareader_engine::session::{AppearanceSnapshot as Held, Session};
use mareader_engine::settings::Tab;
use mareader_engine::study::Action as StudyAction;
use mareader_engine::toast::Toast;
use ui_state::UiState;

/// Settings reach disk this long after a change, at most: the engine's own
/// `save::EVERY`, which a frame is booked just after.
pub const SAVE_EVERY: Duration = Duration::from_millis(500);

/// What a launch says when some choices came back as defaults.
const REPAIRED: &str = "Some settings couldn't be read and were reset to defaults.";

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
    /// The owner: settings, tree, route, and the door that changes them.
    session: Session,
    /// What dies with the window: `MareaderApp` is these two and nothing else.
    ui: UiState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FontState {
    Missing,
    Installed,
    Ready,
}

/// A selection from the options: `text` on `page`, maybe explained.
#[derive(Default)]
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
    /// A skill run on the selection, or on the page at `spot`.
    skill: Option<mareader_settings::Skill>,
    spot: Option<(f32, f32)>,
}

impl Default for MareaderApp {
    fn default() -> Self {
        Self::wired(Machine::default())
    }
}

impl MareaderApp {
    /// An app for a window: what its launch found, on the machine it runs on.
    ///
    /// The store an older build wrote is read here and never again, and the
    /// engine's own files take its place from this launch on.
    pub fn new(cc: &eframe::CreationContext<'_>, host: Shared, machine: Machine) -> Self {
        let loaded = crate::import::once(cc.storage, host.state_dir());
        Self::from_loaded(loaded, &cc.egui_ctx, host, machine)
    }

    /// The same app with no store to read: a harness's, or a measurement's.
    pub fn on(ctx: &Context, host: Shared, machine: Machine) -> Self {
        Self::from_loaded(Loaded::default(), ctx, host, machine)
    }

    /// An app on a host with no machine: a test's, with nothing on disk.
    pub fn wired(machine: Machine) -> Self {
        Self::on(&Context::default(), Arc::new(Fake::default()), machine)
    }

    /// An app whose own two files live in `dir`, read as a launch reads them.
    ///
    /// No service is attached and no store is consulted: a test drives the
    /// window here, and what it wrote at its last exit is what comes back.
    pub fn kept(dir: &std::path::Path) -> Self {
        let host: Shared = Arc::new(Fake::under(dir));
        let loaded = crate::import::once(None, host.state_dir());
        Self::from_loaded(loaded, &Context::default(), host, Machine::default())
    }

    fn built(loaded: Loaded, ctx: &Context, host: Shared, machine: Machine) -> Self {
        let titlebar = TitleBar::default();
        let (wake, waker) = setup::WakeSlot::new(ctx);
        let session = Session::from_loaded(host, waker, loaded);
        let mut app = Self {
            session,
            ui: UiState {
                titlebar,
                platform: Platform::current(),
                chromeless: false,
                menu_shot: false,
                read_aloud_shot: false,
                transcript_shot: Vec::new(),
                trigger_shot: Vec::new(),
                script_shot: Vec::new(),
                scripts: Default::default(),
                page_ai_seed: None,
                word_window_seed: None,
                sidebar: Sidebar::default(),
                settings_window: SettingsWindow::default(),
                find_on_open: None,
                find: FindBar::default(),
                toasts: Toasts::default(),
                study: StudyUi::default(),
                page_ai: crate::page_ai::PageAiUi::default(),
                study_mode: crate::study_mode::StudyModeUi::default(),
                space: crate::space::SpaceUi::default(),
                read_aloud: crate::read_aloud::ReadAloudUi::default(),
                ai: crate::ai::Ai::default(),
                dict: crate::dict::DictUi::default(),
                english: None,
                staged: Default::default(),
                skill_reveal: None,
                lights: NativeLights::default(),
                light_spots: None,
                documents: DocumentsUi::default(),
                files: Files::default(),
                library: LibraryUi::default(),
                outline: OutlineState::default(),
                thumbs: ThumbState::default(),
                word_window: Default::default(),
                shelf_tree: PanelState::default(),
                page_bars: std::collections::BTreeMap::new(),
                pane_rects: Vec::new(),
                lifting: None,
                library_rect: None,
                reopen: (u64::MAX, Vec::new()),
                fade: LookFade::default(),
                styled: None,
                reduced_motion: Arc::new(AtomicBool::new(false)),
                resumed: false,
                fonts: FontState::Missing,
                machine_fonts: MachineFonts::default(),
                queued: Commands::new(),
                deferred_events: Vec::new(),
                control: None,
                wake,
                #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
                live: theme::live::LiveTokens::from_env(),
            },
        };
        app.wire(ctx, &machine);
        app
    }

    /// An app from what a launch found: its choices, what to reopen, a repair.
    ///
    /// Unreadable settings arrive repaired section by section, and the notes
    /// that say so go to stderr and to one toast.
    pub fn from_loaded(loaded: Loaded, ctx: &Context, host: Shared, machine: Machine) -> Self {
        for note in &loaded.notes {
            eprintln!("mareader: {note}");
        }
        let repaired = !loaded.notes.is_empty();
        let mut app = Self::built(loaded, ctx, host, machine);
        if repaired {
            app.ui.toasts.push(Toast::error(REPAIRED));
        }
        app
    }

    /// Answers its dialogs with `dialogs`, as a launch's shell does.
    pub fn with_dialogs(mut self, dialogs: Runner) -> Self {
        self.ui.files.set_dialogs(dialogs);
        self
    }

    /// Attaches a live UI control channel host to this app instance.
    pub fn with_control(mut self, control: crate::control::ControlHost) -> Self {
        self.ui.control = Some(control);
        self
    }

    /// Installs or replaces the live UI control host on an existing app.
    pub fn set_control(&mut self, control: crate::control::ControlHost) {
        self.ui.control = Some(control);
    }
}
