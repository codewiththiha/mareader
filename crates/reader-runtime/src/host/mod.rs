//! The reader host: the workspace owner between the session's runtime and
//! its panes.
//!
//! ```text
//! /reader → ReaderRuntime (session lifecycle)
//!         → ReaderHost    (chrome placement, focus, bounds, workspace commands)
//!         → PaneManager   (pane create / focus / resize / close / dispose_all)
//!         → pane runtime  (one document session each, behind `PaneRuntime`)
//! ```
//!
//! What the host OWNS: the title bar and where the chrome regions sit, the
//! sidebar rail's mount points and the shell controller that coordinates
//! them, the settings modal's placement, the one active pane, pane creation
//! and removal, bounds measurement, the workspace commands (open, Library),
//! the lifecycle dispatch to its panes, and the appearance boundary (the
//! resolved look pushed to every pane).
//!
//! What it does NOT own: any document. It never names a format, an engine
//! type or a pane implementation — the composition root injects a
//! [`contract::PaneFactory`], and the host speaks to what it built only
//! through [`contract::PaneRuntime`]. `tools/check-host-boundary.mjs` holds
//! that line in CI.

pub mod contract;
pub mod manager;
pub mod model;
mod view;

use leptos::prelude::*;
use runtime_contract::boundary::{LaunchDocument, ShellApi};
use serde::Serialize;

use app_ui::components::shell::controller::ShellController;
use contract::{PaneAppearance, PaneCommand, PaneDocStatus, PaneEnv, PaneFactory, PaneSurface};
use manager::PaneManager;
use model::{
    DocumentId, DocumentRef, PaneBounds, PaneError, PaneFormat, PaneId, PaneLifecycle, PaneRequest,
};

pub use view::ReaderHostView;

/// The session slices the composition root hands the host: Copy handles
/// onto the SESSION's own state (never the Shell's live signals).
#[derive(Clone, Copy)]
pub struct HostSession {
    pub runtime: crate::runtime::ReaderRuntime,
    pub settings: RwSignal<reader_core::settings::Settings>,
    pub ui: app_state::UiState,
    pub api: crate::context::ApiHandle,
    pub session_id: u32,
}

/// The reader host. Copy: the title bar's closures, the workspace slot and
/// the session's command entry all capture it.
#[derive(Clone, Copy)]
pub struct ReaderHost {
    session: HostSession,
    manager: PaneManager,
    /// The shared chrome handles this session's chrome code reads. Its
    /// reader surface follows the ACTIVE pane.
    chrome: app_state::ChromeState,
    /// The shell's layout brain for the whole workspace: the title bar, the
    /// traffic lights, the floating label and both rail mount points ask it.
    shell: ShellController,
    /// The settings modal's open signal (the host places the modal; the
    /// active pane fills it).
    settings_open: RwSignal<bool>,
    /// The appearance boundary's resolved motion switches.
    motion: RwSignal<app_state::Motion>,
    /// The workspace slot's measured size, the source of every pane's
    /// bounds.
    slot_size: RwSignal<(f64, f64)>,
}

impl ReaderHost {
    /// Build the host inside the session's reactive owner. `factory` is the
    /// pane implementation the composition root chose.
    pub fn new(session: HostSession, factory: PaneFactory) -> Self {
        let manager = PaneManager::new(factory);
        let settings = session.settings;
        let initial = settings.with_untracked(|s| app_state::Motion::from_prefs(&s.animations));
        let motion = RwSignal::new(initial);

        // What the shared chrome reads about "the reader": the ACTIVE pane's
        // published facts, read through `try_` because a pane's signals die
        // with the pane.
        let reflowable = Signal::derive(move || {
            manager
                .active_pane()
                .and_then(|pane| pane.surface().reflowable.try_get())
                .unwrap_or(false)
        });
        let search_visible = Signal::derive(move || {
            manager
                .active_pane()
                .and_then(|pane| pane.surface().search_visible.try_get())
                .unwrap_or(false)
        });
        let chrome = app_state::ChromeState {
            settings,
            ui: session.ui,
            reader: app_state::ReaderSurface {
                reflowable,
                search_visible,
                sidebar_slide: motion,
            },
        };

        // One controller for the whole workspace, provided for the title bar,
        // the traffic lights, the floating label and both rail mount points.
        // It owns the open/close slide machine, so the chrome stays aligned
        // with the rail's pixels for the whole slide.
        let shell = ShellController::reader(chrome);
        provide_context(shell);

        // The settings modal is opened from several places (the view menu's
        // Settings… item, the sidebar header's gear) that sit under different
        // mount points, so the open signal is shared through context.
        let settings_open = RwSignal::new(false);
        provide_context(settings_open);

        let host = Self {
            session,
            manager,
            chrome,
            shell,
            settings_open,
            motion,
            slot_size: RwSignal::new((0.0, 0.0)),
        };
        host.install_appearance_boundary();
        host.install_bounds();
        host.install_reports();

        // The workspace as the diagnostics surface reports it. The probe
        // reads the manager's plain state, never the arena.
        let probe = manager.shared();
        crate::diagnostics::install_host_probe(move || {
            probe
                .as_ref()
                .and_then(|shared| shared.try_borrow().ok().map(|state| snapshot_of(&state)))
                .unwrap_or_default()
        });
        host
    }

    pub fn manager(&self) -> PaneManager {
        self.manager
    }

    /// The appearance boundary: the motion switches resolve ONCE, here,
    /// from the session's settings copy (which the frame theme keeps
    /// current, cross-frame edits included), and every live pane is handed
    /// the result. Written only on a real change: a Leptos `set` notifies
    /// even when equal.
    fn install_appearance_boundary(&self) {
        let host = *self;
        Effect::new(move |_| {
            let next = host
                .session
                .settings
                .with(|s| app_state::Motion::from_prefs(&s.animations));
            if host.motion.try_get_untracked().is_some_and(|m| m != next) {
                host.motion.set(next);
            }
            let appearance = PaneAppearance { motion: next };
            for pane in host.manager.live_panes() {
                pane.appearance(appearance);
            }
        });
    }

    /// The appearance a pane created now starts with.
    fn appearance_now(&self) -> PaneAppearance {
        PaneAppearance {
            motion: self.motion.try_get_untracked().unwrap_or_default(),
        }
    }

    /// Bounds: the host measures its workspace slot and hands every pane its
    /// box explicitly. No split mode yet, so every pane fills the slot; the
    /// pane owns everything inside its box.
    fn install_bounds(&self) {
        let host = *self;
        let stop = app_chrome::hooks::use_resize_observer::observe_content_size(
            app_chrome::hooks::dom::VIEWER_SLOT_ID,
            self.slot_size,
        );
        on_cleanup(stop);
        Effect::new(move |_| {
            let Some((width, height)) = host.slot_size.try_get() else {
                return;
            };
            // Tracked on the placement too: a pane placed after the last
            // measurement is handed the current box.
            let _ = host.manager.placed();
            untrack(|| {
                host.manager
                    .resize_all(|_| PaneBounds::filling(width, height))
            });
        });
    }

    /// The bounds a pane placed now is handed.
    fn bounds_now(&self) -> PaneBounds {
        let (width, height) = self.slot_size.try_get_untracked().unwrap_or_default();
        PaneBounds::filling(width, height)
    }

    /// The two facts the Shell's probe serves from this session (§21): the
    /// ACTIVE pane's document status, which its route policy answers from,
    /// and its page. Both are PUSHED across the boundary: the Shell holds no
    /// reader state, and the host's scope is what ends the pushing.
    fn install_reports(&self) {
        let host = *self;
        Effect::new(move |_| {
            let Some(surface) = host.active_surface() else {
                // No active pane: nothing is open in the workspace.
                crate::report_status(&host.session.api, PaneDocStatus::Idle.word(), None);
                return;
            };
            // try_: a pane's dispose can wake this effect after the pane's
            // owner is gone — the report is worth nothing then.
            let Some(status) = surface.status.try_get() else {
                return;
            };
            let error = surface.error.try_get().flatten();
            crate::report_status(&host.session.api, status.word(), error);
        });
        Effect::new(move |_| {
            if let Some(page) = host.active_surface().and_then(|s| s.page.try_get()) {
                crate::diagnostics::set_reader_page(page);
            }
        });
    }

    /// The active pane's published facts (tracked on the active id).
    pub fn active_surface(&self) -> Option<PaneSurface> {
        self.manager.active_pane().map(|pane| pane.surface())
    }

    /// The active pane's document status (tracked).
    pub fn active_status(&self) -> PaneDocStatus {
        self.active_surface()
            .and_then(|surface| surface.status.try_get())
            .unwrap_or_default()
    }

    /// The environment a new pane is handed: the session's slices, plus its
    /// view of the focus authority — DERIVED from the one active id.
    fn env_for(&self, id: PaneId) -> PaneEnv {
        let manager = self.manager;
        PaneEnv {
            runtime: self.session.runtime,
            settings: self.session.settings,
            ui: self.session.ui,
            api: self.session.api,
            session_id: self.session.session_id,
            chrome: self.chrome,
            active: Signal::derive(move || manager.active() == Some(id)),
            settings_open: self.settings_open,
        }
    }

    /// Create a pane for `launch` (none: an empty pane waiting for one) and
    /// hand it the current appearance and bounds.
    pub fn create_pane(
        &self,
        launch: Option<LaunchDocument>,
        request_focus: bool,
    ) -> Result<PaneId, PaneError> {
        let request = PaneRequest {
            document: launch.as_ref().and_then(document_ref),
            format: PaneFormat::Pending,
            initial_page: launch.as_ref().map(|l| l.resume_page).unwrap_or(1),
            initial_zoom: None,
            request_focus,
        };
        let host = *self;
        let id = self
            .manager
            .create(request, launch, move |id| host.env_for(id))?;
        if let Some(pane) = self.manager.pane(id) {
            pane.appearance(self.appearance_now());
        }
        self.manager.resize(id, self.bounds_now())?;
        Ok(id)
    }

    /// The workspace's open command (a drop, a dialog, a warm reader's
    /// promotion): the ACTIVE pane opens it in place — the pane keeps its id
    /// and replaces its document session. With no pane, a new one is
    /// created for it.
    pub fn open(&self, launch: LaunchDocument) -> Result<PaneId, PaneError> {
        match self.manager.active_untracked() {
            Some(id) => {
                let pane = self.manager.pane(id).ok_or(PaneError::Gone(id))?;
                pane.command(PaneCommand::Open(Box::new(launch)))?;
                Ok(id)
            }
            None => self.create_pane(Some(launch), true),
        }
    }

    /// A pane asks to become active: the ONE focus authority decides.
    pub fn set_active(&self, id: PaneId) -> Result<(), PaneError> {
        self.manager.set_active(id)
    }

    /// Close one pane inside the live session.
    pub fn close_pane(&self, id: PaneId) -> Result<(), PaneError> {
        self.manager.close(id)
    }

    /// The Library button: every pane writes its durable point and stops its
    /// render work, then the Shell is asked to navigate. The Shell's dispose
    /// comes back over the boundary and tears the workspace down.
    pub fn return_to_library(&self) {
        for pane in self.manager.live_panes() {
            let _ = pane.command(PaneCommand::PrepareLeave);
        }
        self.session.api.navigate_library();
    }

    /// Dispose the workspace: every pane's dispose runs now (explicit,
    /// observable), the host refuses new panes from here. Idempotent. The
    /// panes' async tails wait in [`Self::take_teardown`].
    pub fn dispose(&self) {
        self.manager.dispose_all();
    }

    /// The panes' disposal tails, for the session's runtime to await.
    pub fn take_teardown(&self) -> contract::PaneTeardown {
        self.manager.take_teardown()
    }
}

/// The document identity and address a launch names, as data.
fn document_ref(launch: &LaunchDocument) -> Option<DocumentRef> {
    DocumentId::from_launch(launch.book_id.as_deref(), &launch.path).map(|document_id| {
        DocumentRef {
            document_id,
            path: launch.path.clone(),
        }
    })
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// The workspace as the diagnostics snapshot reports it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSnapshot {
    /// `live` while the host takes panes, `disposed` once its workspace
    /// disposal began.
    pub lifecycle: &'static str,
    pub active_pane: Option<PaneId>,
    /// Every pane not yet `Disposed`, in placement order (a disposing pane
    /// stays listed until its tail finished).
    pub panes: Vec<PaneSnapshot>,
    /// Every pane this host ever created.
    pub panes_created: usize,
}

/// One pane in the snapshot.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneSnapshot {
    pub pane_id: PaneId,
    pub document_id: Option<DocumentId>,
    pub format: PaneFormat,
    pub lifecycle: PaneLifecycle,
    pub focused: bool,
    pub bounds: PaneBounds,
    pub resources: contract::PaneResourceCounts,
}

fn snapshot_of(state: &manager::ManagerState) -> HostSnapshot {
    let core = &state.core;
    let mut panes: Vec<PaneSnapshot> = Vec::new();
    let listed = core.live().iter().copied().chain(core.disposing());
    for id in listed {
        let Some(record) = core.record(id) else {
            continue;
        };
        let runtime = state.panes.get(&id);
        panes.push(PaneSnapshot {
            pane_id: id,
            document_id: match runtime {
                Some(pane) => pane.document(),
                None => record
                    .descriptor
                    .document
                    .as_ref()
                    .map(|d| d.document_id.clone()),
            },
            format: runtime.map_or(record.descriptor.format, |pane| pane.format()),
            lifecycle: record.lifecycle,
            focused: core.active() == Some(id),
            bounds: record.bounds,
            resources: runtime.map(|pane| pane.resources()).unwrap_or_default(),
        });
    }
    HostSnapshot {
        lifecycle: if core.is_host_disposed() {
            "disposed"
        } else {
            "live"
        },
        active_pane: core.active(),
        panes,
        panes_created: core.created(),
    }
}
