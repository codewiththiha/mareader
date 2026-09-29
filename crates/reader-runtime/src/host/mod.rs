//! The reader host: the workspace owner between the session's runtime and
//! its panes.
//!
//! ```text
//! /reader → ReaderRuntime (session lifecycle)
//!         → ReaderHost    (chrome placement, focus, bounds, workspace commands)
//!         → PaneTree      (the layout: splits, ratios, PaneIds — nothing else)
//!         → PaneManager   (pane create / focus / resize / close / dispose_all)
//!         → pane runtime  (one document session each, behind `PaneRuntime`)
//! ```
//!
//! The tree and the manager hold the same set of panes, and the host is the
//! one place that changes both: a pane is created (manager) and then placed
//! (tree), and a closing pane leaves the tree first so the tree can name its
//! successor for the manager's focus hand-over. The layout lays the tree out
//! over the measured workspace slot and every pane is handed its box.
//!
//! What the host OWNS: the title bar and where the chrome regions sit, the
//! sidebar rail's mount points and the shell controller that coordinates
//! them, the settings modal's placement, the one active pane, pane creation
//! and removal, bounds measurement, the workspace commands (open, Library),
//! the lifecycle dispatch to its panes (suspending them while the frame is
//! off screen), and the appearance boundary (the resolved look pushed to
//! every pane).
//!
//! What it does NOT own: any document. It never names a format, an engine
//! type or a pane implementation — the composition root injects a
//! [`contract::PaneFactory`], and the host speaks to what it built only
//! through [`contract::PaneRuntime`]. `tools/check-host-boundary.mjs` holds
//! that line in CI.

pub mod commands;
pub mod contract;
pub mod drag;
pub mod drop_target;
pub mod geometry;
pub mod manager;
pub mod model;
pub mod tree;
mod view;

use std::rc::Rc;

use leptos::prelude::*;
use runtime_contract::boundary::{LaunchDocument, ShellApi};
use runtime_contract::protocol::DocumentDragEvent;
use serde::Serialize;

use app_ui::components::shell::controller::ShellController;
use commands::{DropPlan, WorkspaceCommand};
use contract::{
    OpenRequest, PaneAppearance, PaneClassifier, PaneCommand, PaneDocStatus, PaneEnv, PaneFactory,
    PaneSurface, Placement,
};
use drag::{DocumentDragSource, DragOrigin, DragSession, DropIntent};
use geometry::{DropGeometry, PaneGeometry};
use manager::PaneManager;
use model::{
    DocumentId, DocumentRef, MAX_PANES, PaneBounds, PaneError, PaneFormat, PaneId, PaneLifecycle,
    PaneRequest,
};
use tree::{LayoutNode, PaneTree, Side, SplitAxis, SplitId, TreeLayout};

/// Where the workspace puts a document. Explicit on purpose: there is no
/// hidden "current document" deciding it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenTarget {
    /// The active pane, in place — or, with no pane at all, the workspace's
    /// first pane (the Shell's commands: a drop, a warm reader's launch).
    Active,
    /// This pane, in place: it keeps its id and replaces its document.
    Pane(PaneId),
    /// A new pane beside `of`, split along `axis`, on `side` of it — the
    /// one placement the Split menu, the test hook and a drop all use. The
    /// layout policy refuses it when a half would be under the minimum
    /// pane size ([`tree::split_fits`]).
    Split {
        of: PaneId,
        axis: SplitAxis,
        side: Side,
    },
}

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
    /// Run a closure inside the session's root owner (nothing, once the
    /// session is gone). Work that begins in an event handler — a menu's
    /// split, a drop — has no current owner, and a pane created there would
    /// belong to no scope. A plain `fn`: the host keeps no owner of its own.
    pub enter: fn(&mut dyn FnMut()),
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
    /// The workspace layout: splits and pane ids, nothing heavier.
    tree: RwSignal<PaneTree>,
    /// The one document drag session (see [`drag`]): typed data only, no
    /// DOM reference. Idle whenever no drag is live, and cleared by the
    /// workspace's disposal.
    drag: RwSignal<DragSession>,
    /// The tree laid out over the slot: every pane's box and every
    /// divider's strip. What the bounds effect and the view both follow.
    layout: Memo<TreeLayout>,
    /// A divider drag's latest ratio, waiting for the next frame: pointer
    /// moves only record it, one frame applies the last (see
    /// [`Self::drag_divider`]).
    pending_ratio: StoredValue<Option<(SplitId, f64)>>,
    /// Names a launch's format tag for the descriptor (injected with the
    /// factory: the host reads no extension itself).
    classify: PaneClassifier,
    /// Whether this frame is the one on screen: the host suspends its panes
    /// while it is not.
    frame_active: Signal<bool>,
}

impl ReaderHost {
    /// Build the host inside the session's reactive owner. `factory` is the
    /// pane implementation the composition root chose, `classify` the same
    /// implementation's reading of a document address.
    pub fn new(session: HostSession, factory: PaneFactory, classify: PaneClassifier) -> Self {
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

        let slot_size = RwSignal::new((0.0, 0.0));
        let tree = RwSignal::new(PaneTree::new());
        let drag = RwSignal::new(DragSession::Idle);
        let layout = Memo::new(move |_| {
            let (width, height) = slot_size.get();
            tree.with(|tree| tree.layout(PaneBounds::filling(width, height)))
        });
        let host = Self {
            session,
            manager,
            chrome,
            shell,
            settings_open,
            motion,
            slot_size,
            tree,
            drag,
            layout,
            pending_ratio: StoredValue::new(None),
            classify,
            frame_active: app_chrome::hooks::frame_active::use_frame_active(),
        };
        host.install_appearance_boundary();
        host.install_bounds();
        host.install_suspension();
        host.install_reports();
        host.install_drag();

        // The workspace as the diagnostics surface reports it. The probe
        // reads the manager's plain state, never the arena — and holds it
        // WEAKLY: it lives in a thread-local that outlasts the session, so a
        // strong reference would keep the host's state for as long as the
        // frame waits for its next session. The teardown settles it into
        // plain data ([`Self::take_teardown`]).
        // The layout rides along through the tree's signal handle — an arena
        // key, not a reference: once the session is gone it reads nothing.
        let probe = manager.shared().map(|shared| Rc::downgrade(&shared));
        crate::diagnostics::install_host_probe(move || {
            let shared = probe.as_ref()?.upgrade()?;
            let state = shared.try_borrow().ok()?;
            Some(snapshot_of(&state, layout_of(tree), drag_phase(drag)))
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

    /// Bounds: the host measures its workspace slot, lays the tree out over
    /// it, and hands every pane its box explicitly; the pane owns everything
    /// inside its box. A divider drag is the same path — a new ratio, a new
    /// layout, new boxes — so a pane reacts to it exactly as to a window
    /// resize, through its own viewport and zoom-follow machinery.
    ///
    /// The slot is looked up by its document-wide id ON PURPOSE: it is the
    /// HOST's element (one per session, rendered by the host's own view),
    /// not a pane's — every pane-owned element is looked up inside its
    /// pane's root instead (`crate::pane::dom`).
    fn install_bounds(&self) {
        let host = *self;
        let stop = app_chrome::hooks::use_resize_observer::observe_content_size(
            app_chrome::hooks::dom::VIEWER_SLOT_ID,
            self.slot_size,
        );
        on_cleanup(stop);
        Effect::new(move |_| {
            let Some(layout) = host.layout.try_get() else {
                return;
            };
            // Tracked on the placement too: a pane placed after the last
            // layout is handed the current box.
            let _ = host.manager.placed();
            untrack(|| host.hand_out_bounds(&layout));
        });
    }

    /// Every live pane's box from `layout`. A pane the tree does not hold
    /// (none should exist) fills the slot rather than getting nothing.
    fn hand_out_bounds(&self, layout: &TreeLayout) {
        let whole = self.slot_rect();
        self.manager
            .resize_all(|id| layout.bounds_of(id).unwrap_or(whole));
    }

    /// Re-lay the tree NOW and hand the boxes out: a pane placed by an open
    /// has its box before its view mounts.
    fn relayout_now(&self) {
        let layout = self.layout_now();
        self.hand_out_bounds(&layout);
    }

    /// The tree laid out over the slot as measured now (untracked).
    fn layout_now(&self) -> TreeLayout {
        let rect = self.slot_rect();
        self.tree
            .try_with_untracked(|tree| tree.layout(rect))
            .unwrap_or_default()
    }

    /// The workspace slot as a rect at the origin (the tree's coordinates).
    fn slot_rect(&self) -> PaneBounds {
        let (width, height) = self.slot_size.try_get_untracked().unwrap_or_default();
        PaneBounds::filling(width, height)
    }

    /// The layout (tracked): the view positions pane entries and dividers
    /// by it.
    pub fn layout(&self) -> TreeLayout {
        self.layout.try_get().unwrap_or_default()
    }

    /// A divider drag moved: record the ratio the pointer asks for and
    /// apply the LAST one on the next animation frame. Pointer events can
    /// outpace frames several times over; each applied ratio re-lays the
    /// workspace and hands two panes new boxes, which is what their own
    /// resize handling (viewport measurement, zoom follow, the render
    /// scheduler) absorbs — once per frame, never once per event.
    pub fn drag_divider(&self, split: SplitId, ratio: f64) {
        let queued = self
            .pending_ratio
            .try_update_value(|pending| pending.replace((split, ratio)).is_some())
            .unwrap_or(true);
        if queued {
            return;
        }
        let host = *self;
        request_animation_frame(move || {
            let Some(Some((split, ratio))) = host.pending_ratio.try_update_value(Option::take)
            else {
                return;
            };
            host.tree.try_update(|tree| {
                let _ = tree.set_ratio(split, ratio);
            });
        });
    }

    /// Suspension: while this frame is off screen (a warm reader waiting
    /// behind the shelf, a retiring one being disposed) every placed pane
    /// is `Suspended` — it keeps its document session but takes no new work
    /// — and coming back on screen resumes them. A pane still mounting is
    /// suspended when it becomes ready ([`Self::pane_ready`]); a transition
    /// that does not apply (already there, not ready yet) is nothing to do.
    fn install_suspension(&self) {
        let host = *self;
        Effect::new(move |_| {
            let Some(on_screen) = host.frame_active.try_get() else {
                return;
            };
            for id in untrack(|| host.manager.placed()) {
                let _ = if on_screen {
                    host.manager.resume(id)
                } else {
                    host.manager.suspend(id)
                };
            }
        });
    }

    /// A pane's view is built and its effects installed: `Mounting →
    /// Ready`, then straight on to `Suspended` if the frame is off screen
    /// (a warm session's pane starts parked).
    pub(crate) fn pane_ready(&self, id: PaneId) {
        if self.manager.mark_ready(id).is_err() {
            return;
        }
        if self.frame_active.try_get_untracked() == Some(false) {
            let _ = self.manager.suspend(id);
        }
    }

    /// The box `id` holds now (what its view mounts at).
    pub(crate) fn bounds_now(&self, id: PaneId) -> PaneBounds {
        self.manager
            .bounds_untracked(id)
            .unwrap_or_else(|| self.slot_rect())
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

    /// The environment a new pane is handed: the session's slices, its view
    /// of the focus authority — DERIVED from the one active id — and the
    /// two requests it may make of the host: focus, and open.
    fn env_for(&self, id: PaneId) -> PaneEnv {
        let manager = self.manager;
        let host = *self;
        PaneEnv {
            runtime: self.session.runtime,
            settings: self.session.settings,
            ui: self.session.ui,
            api: self.session.api,
            session_id: self.session.session_id,
            chrome: self.chrome,
            active: Signal::derive(move || manager.active() == Some(id)),
            settings_open: self.settings_open,
            request_focus: manager.focus_request(id),
            // The pane names only itself: the host turns its placement into
            // the explicit target.
            open: Callback::new(move |request: OpenRequest| {
                let target = match request.placement {
                    Placement::Here => OpenTarget::Pane(id),
                    Placement::Beside(axis) => OpenTarget::Split {
                        of: id,
                        axis,
                        side: Side::After,
                    },
                };
                let opened = host.in_session(|| host.open_document(request.launch, target));
                if let Some(Err(error)) = opened {
                    host.refused(error);
                }
            }),
            can_split: Signal::derive(move || manager.placed().len() < MAX_PANES),
        }
    }

    /// Run `work` inside the session's root owner ([`HostSession::enter`]);
    /// `None` when the session is gone and nothing ran.
    fn in_session<R>(&self, work: impl FnOnce() -> R) -> Option<R> {
        let mut work = Some(work);
        let mut out = None;
        (self.session.enter)(&mut || {
            if let Some(work) = work.take() {
                out = Some(work());
            }
        });
        out
    }

    /// Say why the workspace refused a placement: the console for the
    /// record, a toast for the user.
    fn refused(&self, error: PaneError) {
        leptos::logging::warn!("[reader] the workspace refused an open: {error:?}");
        let message = match error {
            PaneError::WorkspaceFull => {
                format!("The workspace holds at most {MAX_PANES} panes.")
            }
            PaneError::Layout(tree::TreeError::NoRoom(_)) => {
                "There is no room to split this pane.".to_string()
            }
            _ => "That document could not be placed.".to_string(),
        };
        let _ = self
            .session
            .ui
            .toast
            .try_set(Some(app_state::state::Toast::new(message)));
    }

    /// The workspace's first pane, for `launch` (none: an empty pane
    /// waiting for one — a warm reader), filling the slot.
    pub fn create_root(&self, launch: Option<LaunchDocument>) -> Result<PaneId, PaneError> {
        if self.tree.try_with_untracked(PaneTree::is_empty) != Some(true) {
            return Err(PaneError::Layout(tree::TreeError::NotEmpty));
        }
        self.create_placed(launch, |tree, id| tree.set_root(id))
    }

    /// Create a pane and place it in the tree, transactionally: the pane is
    /// created (the manager mints its id, and it takes focus), `place` puts
    /// it in the layout, and every pane is handed its new box — or, when
    /// the layout refuses, the pane is closed again and nothing changed.
    fn create_placed(
        &self,
        launch: Option<LaunchDocument>,
        place: impl FnOnce(&mut PaneTree, PaneId) -> Result<(), tree::TreeError>,
    ) -> Result<PaneId, PaneError> {
        let id = self.create_unplaced(launch)?;
        let placed = self
            .tree
            .try_update(|tree| place(tree, id))
            .unwrap_or(Err(tree::TreeError::UnknownPane(id)));
        if let Err(error) = placed {
            let _ = self.manager.close(id, None);
            return Err(PaneError::Layout(error));
        }
        self.relayout_now();
        Ok(id)
    }

    /// Create a pane for `launch` and hand it the current appearance. Not
    /// yet placed: [`Self::create_placed`] is the only caller.
    fn create_unplaced(&self, launch: Option<LaunchDocument>) -> Result<PaneId, PaneError> {
        let format = launch
            .as_ref()
            .filter(|launch| !launch.path.is_empty())
            .map_or(PaneFormat::Pending, |launch| (self.classify)(&launch.path));
        let request = PaneRequest {
            document: launch.as_ref().and_then(document_ref),
            format,
            initial_page: launch.as_ref().map(|l| l.resume_page).unwrap_or(1),
            initial_zoom: None,
            // A pane the user just opened is the one they look at.
            request_focus: true,
        };
        let host = *self;
        let id = self
            .manager
            .create(request, launch, move |id| host.env_for(id))?;
        if let Some(pane) = self.manager.pane(id) {
            pane.appearance(self.appearance_now());
        }
        Ok(id)
    }

    /// The workspace's document placement: `launch` goes where `target`
    /// says (see [`OpenTarget`]).
    ///
    /// * In place ([`OpenTarget::Pane`], or the active pane): the pane keeps
    ///   its id and replaces its document session — the one per-pane
    ///   document change; no other pane is touched.
    /// * Split: a NEW pane with its own session, split off the named one;
    ///   it takes focus. Transactional: a refused placement leaves the
    ///   workspace as it was. A document that then fails to load leaves the
    ///   new pane showing its error, closable like any other.
    pub fn open_document(
        &self,
        launch: LaunchDocument,
        target: OpenTarget,
    ) -> Result<PaneId, PaneError> {
        match target {
            OpenTarget::Active => match self.manager.active_untracked() {
                Some(id) => self.open_in(id, launch),
                None => self.create_root(Some(launch)),
            },
            OpenTarget::Pane(id) => self.open_in(id, launch),
            OpenTarget::Split { of, axis, side } => {
                let known = self.tree.try_with_untracked(|tree| tree.contains(of));
                if known != Some(true) {
                    return Err(PaneError::Layout(tree::TreeError::UnknownPane(of)));
                }
                commands::check_room(&self.layout_now(), of, axis)?;
                self.create_placed(Some(launch), |tree, id| {
                    tree.split(of, axis, side, id).map(|_| ())
                })
            }
        }
    }

    /// Open `launch` in pane `id`, in place.
    ///
    /// A suspended pane takes no work, and an open IS work: it is resumed
    /// for the command (every gate of the open is passed synchronously, in
    /// the command itself) and parked again if the frame is still off
    /// screen. The promotion's slot flip normally resumed it already.
    fn open_in(&self, id: PaneId, launch: LaunchDocument) -> Result<PaneId, PaneError> {
        let pane = self.manager.pane(id).ok_or(PaneError::Gone(id))?;
        let parked = self.manager.lifecycle(id) == Some(PaneLifecycle::Suspended);
        if parked {
            self.manager.resume(id)?;
        }
        let sent = pane.command(PaneCommand::Open(Box::new(launch)));
        if parked && self.frame_active.try_get_untracked() == Some(false) {
            let _ = self.manager.suspend(id);
        }
        sent.map(|()| id)
    }

    /// A pane asks to become active: the ONE focus authority decides.
    pub fn set_active(&self, id: PaneId) -> Result<(), PaneError> {
        self.manager.set_active(id)
    }

    /// Close one pane inside the live session. It leaves the layout first
    /// (its split collapses into the sibling), and the layout names the
    /// pane nearest to it as the focus successor; then the manager disposes
    /// it — only it: every other pane keeps its session.
    pub fn close_pane(&self, id: PaneId) -> Result<(), PaneError> {
        match self.manager.lifecycle(id) {
            None => return Err(PaneError::Unknown(id)),
            Some(lifecycle) if !lifecycle.is_live() => return Err(PaneError::Gone(id)),
            Some(_) => {}
        }
        let successor = self
            .tree
            .try_update(|tree| tree.remove(id).ok().flatten())
            .flatten();
        self.manager.close(id, successor)?;
        self.relayout_now();
        Ok(())
    }

    /// How many panes the workspace shows (tracked).
    pub fn pane_count(&self) -> usize {
        self.manager.placed().len()
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
        // No drag outlives the workspace: whatever was in flight ends here,
        // with no drop. Untracked — the view is being torn down, and the
        // listeners go with the owner.
        self.drag.try_update_untracked(|session| session.cancel());
        self.manager.dispose_all();
        // The layout lets go of the panes with the manager. Untracked: the
        // workspace view is being torn down, not re-laid out.
        self.tree
            .try_update_untracked(|tree| *tree = PaneTree::new());
    }

    /// The panes' disposal tails, for the session's runtime to await. When
    /// the last one finished, the diagnostics probe is settled into the
    /// workspace's final snapshot and the manager's state is let go — the
    /// only reference this future keeps, and only until then.
    pub fn take_teardown(&self) -> contract::PaneTeardown {
        let shared = self.manager.shared();
        let tails = self.manager.take_teardown();
        // The drag as the dispose left it, read now, while the session's
        // arena still holds it (the settled snapshot outlives the session):
        // proof that no drag outlived the workspace, not an assumption.
        let drag = drag_phase(self.drag);
        Box::pin(async move {
            tails.await;
            let last = shared.as_ref().and_then(|shared| {
                shared
                    .try_borrow()
                    .ok()
                    .map(|state| snapshot_of(&state, None, drag))
            });
            if let Some(last) = last {
                crate::diagnostics::settle_host_probe(last);
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Document drag and drop
// ---------------------------------------------------------------------------

impl ReaderHost {
    /// The drag session's lifetime. While a drag is live, ONE window listener
    /// set exists — Escape (capture phase, so no pane shortcut sees it) and
    /// the window losing focus, both cancelling — installed when the drag
    /// goes live and removed when it ends, or with the host's owner. A drag
    /// also ends with no drop when the frame leaves the screen (a route
    /// transition) or when the pane it was lifted from goes away.
    fn install_drag(&self) {
        let host = *self;
        let live = Memo::new(move |_| host.drag.with(DragSession::is_live));
        Effect::new(move |_| {
            if live.get() {
                install_drag_listeners(host);
            }
        });
        let source = Memo::new(move |_| host.drag.with(DragSession::source_pane));
        Effect::new(move |_| {
            let on_screen = host.frame_active.try_get().unwrap_or(true);
            let placed = host.manager.placed();
            let gone = source.get().is_some_and(|pane| !placed.contains(&pane));
            if !on_screen || gone {
                untrack(|| host.cancel_drag());
            }
        });
    }

    /// The pending drop's preview (tracked; `None` while nothing is aimed).
    pub fn drag_preview(&self) -> Option<drag::Preview> {
        self.drag.try_with(DragSession::preview).flatten()
    }

    /// A drag is live (untracked).
    pub fn drag_live(&self) -> bool {
        self.drag
            .try_with_untracked(DragSession::is_live)
            .unwrap_or(false)
    }

    /// A press on pane `id`'s drag handle, at client `at`. Arms only: the
    /// geometry is measured once the pointer passes the threshold.
    pub fn drag_press(&self, id: PaneId, at: (f64, f64)) -> bool {
        let Some(source) = self.pane_source(id) else {
            return false;
        };
        self.drag
            .try_maybe_update(|session| {
                let armed = session.arm(source, at);
                (armed, armed)
            })
            .unwrap_or(false)
    }

    /// The pointer of a live drag moved to client `at`. Subscribers hear of
    /// it only when the shown target (or the phase) changed.
    pub fn drag_move(&self, at: (f64, f64)) {
        let host = *self;
        self.drag.try_maybe_update(|session| {
            let changed = session.moved(at, || host.measure_geometry());
            (changed, ())
        });
    }

    /// The pointer was released (at client `at`, when it has a position):
    /// the drag ends, and a drop over a target runs as a workspace command.
    pub fn drag_release(&self, at: Option<(f64, f64)>) {
        let intent = self
            .drag
            .try_maybe_update(|session| {
                let was = session.is_live();
                (was, session.release(at))
            })
            .flatten();
        if let Some(intent) = intent {
            self.commit_drop(intent, None);
        }
    }

    /// End the drag with no drop. Returns whether one was live.
    pub fn cancel_drag(&self) -> bool {
        self.drag
            .try_maybe_update(|session| {
                let was = session.cancel();
                (was, was)
            })
            .unwrap_or(false)
    }

    /// The keyboard's placement from pane `id`'s drag handle: a drag that
    /// starts already aimed at the first split the pane offers.
    pub fn drag_keyboard_start(&self, id: PaneId) -> bool {
        let Some(source) = self.pane_source(id) else {
            return false;
        };
        let Some(geometry) = self.measure_geometry() else {
            return false;
        };
        self.drag
            .try_maybe_update(|session| {
                if !session.start(source, geometry) {
                    return (false, false);
                }
                let _ = drop_target::Edge::PRIORITY
                    .into_iter()
                    .find(|edge| session.select_edge(*edge));
                (true, true)
            })
            .unwrap_or(false)
    }

    /// The keyboard's step: aim at `edge` of the pane currently aimed at.
    pub fn drag_keyboard_edge(&self, edge: drop_target::Edge) {
        self.drag.try_maybe_update(|session| {
            let changed = session.select_edge(edge);
            (changed, ())
        });
    }

    /// A drag step the Shell relays (a document it carried over from the
    /// library). The reader runs the SAME session: its own geometry, its
    /// own targets, its own command.
    pub fn document_drag(&self, event: DocumentDragEvent) {
        match event {
            DocumentDragEvent::Begin { source, x, y } => {
                let descriptor = *source;
                let source = DocumentDragSource {
                    document: DocumentId::from_launch(
                        descriptor.book_id.as_deref(),
                        &descriptor.path,
                    ),
                    format: (self.classify)(&descriptor.path),
                    label: descriptor.label.clone(),
                    origin: DragOrigin::Library(descriptor),
                };
                let Some(geometry) = self.measure_geometry() else {
                    return;
                };
                self.drag.try_maybe_update(|session| {
                    session.cancel();
                    session.start(source, geometry);
                    session.moved((x, y), || None);
                    (true, ())
                });
            }
            DocumentDragEvent::Over { x, y } => self.drag_move((x, y)),
            DocumentDragEvent::Drop { x, y, launch } => {
                let intent = self
                    .drag
                    .try_maybe_update(|session| {
                        let was = session.is_live();
                        (was, session.release(Some((x, y))))
                    })
                    .flatten();
                match intent {
                    Some(intent) => self.commit_drop(intent, Some(*launch)),
                    None => self.carried_drag_abandoned(),
                }
            }
            DocumentDragEvent::Cancel => {
                self.cancel_drag();
                self.carried_drag_abandoned();
            }
        }
    }

    /// A drag the Shell carried here ended with nothing dropped. A workspace
    /// that shows no document was revealed for that drag alone: there is
    /// nothing to read here, so it goes back to the shelf (the Shell takes
    /// the library session it kept back).
    fn carried_drag_abandoned(&self) {
        let reading = self
            .manager
            .live_panes()
            .iter()
            .any(|pane| pane.document().is_some());
        if !reading {
            self.return_to_library();
        }
    }

    /// Carry out a drop through the one workspace command.
    fn commit_drop(&self, intent: DropIntent, resolved: Option<LaunchDocument>) {
        let DropIntent { source, target } = intent;
        let command = WorkspaceCommand::OpenInDropTarget { source, target };
        if let Some(Err(error)) = self.in_session(|| self.run(command, resolved)) {
            self.refused(error);
        }
    }

    /// Run a workspace command. The drop is validated against the workspace
    /// as it is NOW ([`commands::plan`]; a refusal changes nothing), the
    /// source is resolved through the established open path — a pane's
    /// "view again" launch, or the launch the Shell's store resolved for a
    /// library row (`resolved`) — and the placement is the host's one
    /// placement path: the manager creates the pane (it takes focus), the
    /// tree places it, every pane gets its box.
    pub fn run(
        &self,
        command: WorkspaceCommand,
        resolved: Option<LaunchDocument>,
    ) -> Result<PaneId, PaneError> {
        let WorkspaceCommand::OpenInDropTarget { source, target } = command;
        let layout = self.layout_now();
        let live = untrack(|| self.manager.placed().len());
        let plan = self
            .tree
            .try_with_untracked(|tree| {
                commands::plan(target, tree, &layout, live, |id| self.pane_is_empty(id))
            })
            .unwrap_or(Err(PaneError::HostDisposed))?;
        let launch = self.resolve_source(&source, resolved)?;
        match plan {
            DropPlan::Split { of, axis, side } => {
                self.open_document(launch, OpenTarget::Split { of, axis, side })
            }
            DropPlan::Here { pane } => self.open_document(launch, OpenTarget::Pane(pane)),
        }
    }

    /// The dragged document as a launch.
    fn resolve_source(
        &self,
        source: &DocumentDragSource,
        resolved: Option<LaunchDocument>,
    ) -> Result<LaunchDocument, PaneError> {
        match &source.origin {
            DragOrigin::Pane(id) => self
                .manager
                .pane(*id)
                .and_then(|pane| pane.duplicate_launch())
                .ok_or(PaneError::Gone(*id)),
            DragOrigin::Library(descriptor) => Ok(resolved.unwrap_or_else(|| LaunchDocument {
                book_id: descriptor.book_id.clone(),
                path: descriptor.path.clone(),
                resume_page: 1,
                saved_fraction: None,
                blend_override: false,
                cover_data_url: None,
                display_name: Some(descriptor.label.clone()),
            })),
        }
    }

    /// Pane `id` holds no document (a warm reader's empty root).
    fn pane_is_empty(&self, id: PaneId) -> bool {
        self.manager
            .pane(id)
            .is_none_or(|pane| pane.document().is_none())
    }

    /// Pane `id`'s document as a drag source (none while it has none).
    fn pane_source(&self, id: PaneId) -> Option<DocumentDragSource> {
        let pane = self.manager.pane(id)?;
        let document = pane.document()?;
        let format = pane.format();
        Some(DocumentDragSource {
            document: Some(document),
            origin: DragOrigin::Pane(id),
            format,
            label: drag::pane_source_label(format),
        })
    }

    /// The drag's one measurement: the workspace slot's client rect, read
    /// from the DOM here and nowhere else, and every placed pane's box from
    /// the layout the panes are drawn at.
    fn measure_geometry(&self) -> Option<DropGeometry> {
        let slot = web_sys::window()?
            .document()?
            .get_element_by_id(app_chrome::hooks::dom::VIEWER_SLOT_ID)?;
        let rect = slot.get_bounding_client_rect();
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return None;
        }
        let layout = self.layout.try_get_untracked()?;
        let panes = layout
            .panes
            .iter()
            .filter_map(|(id, bounds)| {
                let pane = self.manager.pane(*id)?;
                Some(PaneGeometry {
                    pane: *id,
                    rect: *bounds,
                    format: pane.format(),
                    empty: pane.document().is_none(),
                })
            })
            .collect();
        Some(DropGeometry {
            workspace: PaneBounds {
                x: rect.left(),
                y: rect.top(),
                width: rect.width(),
                height: rect.height(),
            },
            panes,
            can_add: untrack(|| self.manager.placed().len()) < MAX_PANES,
        })
    }
}

/// The live drag's window listeners (see [`ReaderHost::install_drag`]):
/// added now, removed by the calling effect's cleanup. The closures park in
/// owner-scoped storage (a cleanup must be `Send + Sync`, a `Closure` is
/// neither) and hold only the host's Copy handles.
fn install_drag_listeners(host: ReaderHost) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    let Some(window) = web_sys::window() else {
        return;
    };
    let on_key =
        Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(move |event: web_sys::KeyboardEvent| {
            if event.key() == "Escape" && host.cancel_drag() {
                event.prevent_default();
                event.stop_immediate_propagation();
            }
        });
    let on_blur = Closure::<dyn Fn()>::new(move || {
        host.cancel_drag();
    });
    let _ = window.add_event_listener_with_callback_and_bool(
        "keydown",
        on_key.as_ref().unchecked_ref(),
        true,
    );
    let _ = window.add_event_listener_with_callback("blur", on_blur.as_ref().unchecked_ref());
    let parked = StoredValue::new_local(Some((on_key, on_blur)));
    on_cleanup(move || {
        let Some(Some((on_key, on_blur))) = parked.try_update_value(Option::take) else {
            return;
        };
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback_and_bool(
                "keydown",
                on_key.as_ref().unchecked_ref(),
                true,
            );
            let _ = window
                .remove_event_listener_with_callback("blur", on_blur.as_ref().unchecked_ref());
        }
    });
}

/// The document as a drag reports it.
fn drag_phase(drag: RwSignal<DragSession>) -> &'static str {
    drag.try_with_untracked(DragSession::phase)
        .unwrap_or("idle")
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
    /// The workspace layout: the split tree over pane ids (`None` when no
    /// pane is placed, or once the session is gone).
    pub layout: Option<LayoutNode>,
    /// Every pane not yet `Disposed`, in placement order (a disposing pane
    /// stays listed until its tail finished).
    pub panes: Vec<PaneSnapshot>,
    /// Every pane this host ever created.
    pub panes_created: usize,
    /// The document drag session's phase (`idle` whenever no drag is live;
    /// always `idle` once the workspace is gone).
    pub drag: &'static str,
}

/// One pane in the snapshot.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneSnapshot {
    pub pane_id: PaneId,
    pub document_id: Option<DocumentId>,
    pub format: PaneFormat,
    pub lifecycle: PaneLifecycle,
    /// The one active pane (inactive is not disposed: an inactive pane is
    /// listed live, keeps its session and is shown).
    pub focused: bool,
    pub bounds: PaneBounds,
    /// The pane's box in CSS px² — with its zoom, what its raster demand
    /// scales with.
    pub viewport_area: f64,
    pub resources: contract::PaneResourceCounts,
}

/// The layout the tree holds now, if the session still does.
fn layout_of(tree: RwSignal<PaneTree>) -> Option<LayoutNode> {
    tree.try_with_untracked(|tree| tree.root().cloned())
        .flatten()
}

fn snapshot_of(
    state: &manager::ManagerState,
    layout: Option<LayoutNode>,
    drag: &'static str,
) -> HostSnapshot {
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
            viewport_area: record.bounds.width * record.bounds.height,
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
        layout,
        panes,
        panes_created: core.created(),
        drag,
    }
}
