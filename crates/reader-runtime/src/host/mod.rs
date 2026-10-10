//! The workspace owner between the session's runtime and its panes.

pub mod commands;
pub mod contract;
pub mod drag;
pub mod drop_target;
pub mod geometry;
#[cfg(target_arch = "wasm32")]
pub(crate) mod grab;
pub mod library;
pub mod lift;
pub mod manager;
pub mod model;
pub mod theme;
pub mod tree;
mod view;

use std::rc::Rc;

use leptos::prelude::*;
use runtime_contract::boundary::{LaunchDocument, ShellApi};
use serde::Serialize;

use app_ui::components::shell::controller::ShellController;
use commands::{DropPlan, WorkspaceCommand};
use contract::{
    LiftPhase, LiftStep, OpenRequest, PaneAppearance, PaneClassifier, PaneCommand, PaneDocStatus,
    PaneEnv, PaneFactory, PaneSurface, Placement, WorkspaceLook,
};
use drag::{DocumentDragSource, DragSession, DropIntent};
use drop_target::Edge;
use geometry::{DropGeometry, PaneGeometry};
use manager::PaneManager;
use model::{
    DocumentId, DocumentRef, MAX_PANES, PaneBounds, PaneError, PaneFormat, PaneId, PaneLifecycle,
    PaneRequest,
};
use tree::{LayoutNode, MoveDirection, PaneTree, Side, SplitAxis, SplitId, TreeLayout};

/// Where the workspace puts a document; explicit, never a hidden current.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenTarget {
    /// The active pane, in place; with no pane at all, the first one.
    Active,
    /// This pane, in place: it keeps its id and replaces its document.
    Pane(PaneId),
    /// A new pane beside `of`, split along `axis`, on `side` of it.
    Split {
        of: PaneId,
        axis: SplitAxis,
        side: Side,
    },
}

pub use view::ReaderHostView;

/// The session slices the composition root hands the host as Copy handles.
#[derive(Clone, Copy)]
pub struct HostSession {
    pub runtime: crate::runtime::ReaderRuntime,
    pub settings: RwSignal<reader_core::settings::Settings>,
    pub ui: app_state::UiState,
    pub api: crate::context::ApiHandle,
    pub session_id: u32,
    /// Run a closure inside the session's root owner (nothing once gone).
    pub enter: fn(&mut dyn FnMut()),
}

/// The reader host. Copy: the chrome, the slot and the command entry
/// capture it.
#[derive(Clone, Copy)]
pub struct ReaderHost {
    session: HostSession,
    manager: PaneManager,
    /// The shared chrome handles; its reader surface follows the ACTIVE pane.
    chrome: app_state::ChromeState,
    /// The shell's layout brain: the title bar and both rail mounts ask it.
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
    /// Whether SOME pane (not just the focused one) holds a PDF: the
    /// workspace-wide blend gate.
    has_pdf: Signal<bool>,
    /// Closed panes still finishing teardown; their entries stay hidden
    /// until the tail resolves.
    retiring: RwSignal<Vec<PaneId>>,
    /// A divider is being dragged (the pane frames let the pointer through).
    resizing: RwSignal<bool>,
    /// The one document drag session (see [`drag`]): typed data only.
    drag: RwSignal<DragSession>,
    /// A pane lifted by a press-and-hold (see [`lift`]), as plain data.
    lift: RwSignal<Option<lift::Lift>>,
    /// The tree laid out over the slot: pane boxes and divider strips.
    layout: Memo<TreeLayout>,
    /// A divider drag's latest ratio, applied on the next frame.
    pending_ratio: StoredValue<Option<(SplitId, f64)>>,
    /// Names a launch's format tag for the descriptor (injected with the
    /// factory).
    classify: PaneClassifier,
    /// Whether this frame is on screen; off screen its panes suspend.
    frame_active: Signal<bool>,
    /// The rail's Library panel state (see [`library`]), the host's so it
    /// survives a remount.
    library: library::LibraryState,
    /// The workspace's per-pane look state (see [`theme`]).
    themes: theme::PaneThemes,
}

/// The tree laid out over `rect`, as if a lifted pane were closed.
fn lay_out(tree: &PaneTree, rect: PaneBounds, lifted: Option<PaneId>) -> TreeLayout {
    let full = tree.layout(rect);
    let Some(pane) = lifted.filter(|pane| tree.contains(*pane) && tree.len() > 1) else {
        return full;
    };
    let mut rest = tree.clone();
    if rest.remove(pane).is_err() {
        return full;
    }
    let mut layout = rest.layout(rect);
    if let Some(home) = full.bounds_of(pane) {
        layout.panes.push((pane, home));
    }
    layout
}

/// A pane's visible box: half a gap at dividers, the full gap at the
/// perimeter.
fn inset_pane_bounds(bounds: PaneBounds, workspace: PaneBounds, gap: u8) -> PaneBounds {
    let full = gap as f64;
    let half = full / 2.0;
    let has_left_divider = bounds.x > workspace.x;
    let has_right_divider = bounds.x + bounds.width < workspace.x + workspace.width;
    let has_top_divider = bounds.y > workspace.y;
    let has_bottom_divider = bounds.y + bounds.height < workspace.y + workspace.height;
    let (left, right) = side_insets(
        bounds.width,
        has_left_divider,
        has_right_divider,
        half,
        full,
    );
    let (top, bottom) = side_insets(
        bounds.height,
        has_top_divider,
        has_bottom_divider,
        half,
        full,
    );
    PaneBounds {
        x: bounds.x + left,
        y: bounds.y + top,
        width: (bounds.width - left - right).max(0.0),
        height: (bounds.height - top - bottom).max(0.0),
    }
}

/// Divider-facing sides get half a gap, boundary sides the full gap.
fn side_insets(
    size: f64,
    before_divider: bool,
    after_divider: bool,
    half_gap: f64,
    outer_gap: f64,
) -> (f64, f64) {
    let before = if before_divider { half_gap } else { outer_gap };
    let after = if after_divider { half_gap } else { outer_gap };
    let requested = before + after;
    let scale = if requested > size && requested > 0.0 {
        size / requested
    } else {
        1.0
    };
    (before * scale, after * scale)
}

impl ReaderHost {
    /// Build the host inside the session's reactive owner.
    pub fn new(session: HostSession, factory: PaneFactory, classify: PaneClassifier) -> Self {
        let manager = PaneManager::new(factory);
        let settings = session.settings;
        let initial = settings.with_untracked(|s| app_state::Motion::from_prefs(&s.animations));
        let motion = RwSignal::new(initial);
        // Independent themes need the split they serve (see `theme`).
        let panes = Signal::derive(move || manager.placed().len());
        let themes = theme::PaneThemes::new(
            RwSignal::new(settings.with_untracked(|s| s.workspace.independent_themes)),
            RwSignal::new(settings.with_untracked(|s| s.workspace.independent_textures)),
            Signal::derive(move || settings.with(|s| s.workspace.shared_base_mode)),
            panes,
        );

        // What the shared chrome reads: the ACTIVE pane's published facts.
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
        // The blend gate is a workspace fact: SOME pane holds a PDF.
        let has_pdf = Signal::derive(move || {
            manager.placed().iter().any(|id| {
                manager.pane(*id).is_some_and(|pane| {
                    let surface = pane.surface();
                    surface.reflowable.try_get() == Some(false)
                        && surface.status.try_get().is_some_and(|s| s.holds_document())
                })
            })
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

        // One controller for the whole workspace, provided to the chrome; it
        // owns the slide machine.
        let shell = ShellController::reader(chrome);
        provide_context(shell);

        // The settings modal opens from several places, so the signal is
        // shared through context.
        let settings_open = RwSignal::new(false);
        provide_context(settings_open);

        let slot_size = RwSignal::new((0.0, 0.0));
        let tree = RwSignal::new(PaneTree::new());
        let drag = RwSignal::new(DragSession::Idle);
        let lift = RwSignal::new(None::<lift::Lift>);
        // Only WHICH pane is held reshapes the workspace, not every pointer
        // move of the hold.
        let lifted_pane = Memo::new(move |_| lift.with(|l| l.map(|l| l.pane)));
        let layout = Memo::new(move |_| {
            let (width, height) = slot_size.get();
            let lifted = lifted_pane.get();
            tree.with(|tree| lay_out(tree, PaneBounds::filling(width, height), lifted))
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
            has_pdf,
            retiring: RwSignal::new(Vec::new()),
            resizing: RwSignal::new(false),
            drag,
            lift,
            layout,
            pending_ratio: StoredValue::new(None),
            classify,
            frame_active: app_chrome::hooks::frame_active::use_frame_active(),
            library: library::LibraryState::new(),
            themes,
        };
        // The Library panel is the host's; the rail finds it through context.
        provide_context(library::LibraryPanel::new(host));
        // The theme handle is shared through context too: the Settings modal
        // flips the same toggle.
        provide_context(host.theme_handle());
        host.install_appearance_boundary();
        host.install_bounds();
        host.install_suspension();
        host.install_reports();
        host.install_drag();

        // The diagnostics probe reads the manager's plain state and holds it
        // WEAKLY.
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

    /// The appearance boundary: resolve the motion switches once, hand every
    /// live pane its look.
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
            // The per-pane look map's token, tracked with settings: each pane
            // gets ITS look.
            host.themes.version().with(|_| ());
            let global = host.session.settings.with(|s| s.appearance);
            for pane in host.manager.live_panes() {
                pane.appearance(PaneAppearance {
                    motion: next,
                    look: host.themes.look_for(pane.id(), global),
                });
            }
        });
    }

    /// The appearance a pane created now starts with: motion plus its
    /// seeded look.
    fn appearance_now(&self, id: PaneId) -> PaneAppearance {
        let global = self.session.settings.with(|s| s.appearance);
        PaneAppearance {
            motion: self.motion.try_get_untracked().unwrap_or_default(),
            look: self.themes.look_for(id, global),
        }
    }

    /// The appearance menu's theme handle (see [`theme`]).
    pub fn theme_handle(&self) -> app_ui::appearance::ThemeHandle {
        theme::theme_handle(self.themes, self.manager, self.session.settings)
    }

    /// Bounds: measure the slot, lay the tree out, hand each pane its box.
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
            // Tracked on the placement and gap: a later placement gets a box.
            let _ = host.manager.placed();
            let gap = host.session.settings.with(|s| s.workspace.pane_gap);
            untrack(|| host.hand_out_bounds(&layout, gap));
        });
    }

    /// Every live pane's box from `layout`; a pane the tree lacks fills the
    /// slot.
    fn hand_out_bounds(&self, layout: &TreeLayout, gap: u8) {
        let whole = self.slot_rect();
        let split = self.manager.placed().len() > 1;
        self.manager.resize_all(|id| {
            let bounds = layout.bounds_of(id).unwrap_or(whole);
            inset_pane_bounds(bounds, whole, if split { gap } else { 0 })
        });
    }

    /// Re-lay the tree now and hand the boxes out, before a view mounts.
    fn relayout_now(&self) {
        let layout = self.layout_now();
        let gap = self
            .session
            .settings
            .with_untracked(|s| s.workspace.pane_gap);
        self.hand_out_bounds(&layout, gap);
    }

    /// The tree laid out over the slot as measured now (untracked).
    fn layout_now(&self) -> TreeLayout {
        let rect = self.slot_rect();
        let lifted = self
            .lift
            .try_with_untracked(|l| l.map(|l| l.pane))
            .flatten();
        self.tree
            .try_with_untracked(|tree| lay_out(tree, rect, lifted))
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

    /// A divider drag: record the ratio and apply the last one per frame.
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

    /// Suspension: off screen, every placed pane is `Suspended`; on screen,
    /// resumed.
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

    /// A pane's view is built: ready, then suspended if the frame is off.
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

    /// The two facts the Shell's probe serves (§21): the active pane's
    /// status and page, pushed.
    fn install_reports(&self) {
        let host = *self;
        Effect::new(move |_| {
            let Some(surface) = host.active_surface() else {
                // No active pane: nothing is open in the workspace.
                crate::report_status(&host.session.api, PaneDocStatus::Idle.word(), None);
                return;
            };
            // try_: a pane's dispose can wake this after its owner is gone.
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
    fn active_surface(&self) -> Option<PaneSurface> {
        self.manager.active_pane().map(|pane| pane.surface())
    }

    /// The active pane's document status (tracked).
    pub fn active_status(&self) -> PaneDocStatus {
        self.active_surface()
            .and_then(|surface| surface.status.try_get())
            .unwrap_or_default()
    }

    /// The environment a new pane is handed: the session's slices, its
    /// focus view and open.
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
            moves: Signal::derive(move || {
                host.tree
                    .try_with(|tree| tree.moves_for(id))
                    .unwrap_or_default()
            }),
            relocate: Callback::new(move |direction| {
                if let Some(Err(error)) = host.in_session(|| host.move_pane(id, direction)) {
                    leptos::logging::warn!("[reader] pane move refused: {error:?}");
                }
            }),
            workspace: Signal::derive(move || host.workspace_look()),
            lift: Callback::new(move |step| host.lift_step(id, step)),
        }
    }

    /// The workspace facts the shared CSS keys off, tracked: blend,
    /// independent looks, split, style.
    pub fn workspace_look(&self) -> WorkspaceLook {
        let has_pdf = self.has_pdf();
        let independent = self.themes.active().get();
        let split = self.pane_count() > 1;
        self.session.settings.with(|s| {
            let workspace = &s.workspace;
            let color = workspace
                .pane_outline_color
                .resolve(&workspace.pane_outline_custom)
                .unwrap_or("var(--color-accent)");
            let radius = if workspace.pane_corners == reader_core::settings::PaneCorners::Rounded {
                "10px"
            } else {
                "0px"
            };
            let shadow = if workspace.pane_shadow {
                "0 5px 18px rgb(0 0 0 / 0.24)"
            } else {
                "none"
            };
            WorkspaceLook {
                blend: s.layout.blend_mode && has_pdf,
                independent,
                split,
                page_shadow: s.layout.page_shadow,
                style: format!(
                    "--pane-outline-width:{}px;--pane-outline-color:{color};\
                     --pane-corner-radius:{radius};--pane-box-shadow:{shadow}",
                    workspace.pane_outline_width
                ),
            }
        })
    }

    /// A lift step from inside a pane, mapped into the slot's coordinates.
    fn lift_step(&self, id: PaneId, step: LiftStep) {
        let origin = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(app_chrome::hooks::dom::VIEWER_SLOT_ID))
            .map(|slot| {
                let rect = slot.get_bounding_client_rect();
                (rect.left(), rect.top())
            })
            .unwrap_or_default();
        let at = (step.at.0 - origin.0, step.at.1 - origin.1);
        match step.phase {
            LiftPhase::Start => {
                self.begin_lift(id, at);
            }
            LiftPhase::Move => self.lift_move(at),
            LiftPhase::End => self.end_lift(true),
            LiftPhase::Cancel => self.end_lift(false),
        }
    }

    /// Run `work` inside the session's root owner; `None` when it is gone.
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

    /// Say why the workspace refused a placement: the console, a toast.
    fn refused(&self, error: PaneError) {
        leptos::logging::warn!("[reader] the workspace refused an open: {error:?}");
        let message = match error {
            PaneError::WorkspaceFull => {
                format!("The workspace holds at most {MAX_PANES} panes.")
            }
            PaneError::Layout(tree::TreeError::NoRoom(_)) => {
                "There is no room to split this pane.".to_string()
            }
            PaneError::Layout(tree::TreeError::NoMove(_) | tree::TreeError::InvalidMove) => {
                "That pane cannot move there.".to_string()
            }
            _ => "That document could not be placed.".to_string(),
        };
        let _ = self
            .session
            .ui
            .toast
            .try_set(Some(app_state::state::Toast::new(message)));
    }

    /// The workspace's first pane, filling the slot; `None` the empty entry.
    pub fn create_root(&self, launch: Option<LaunchDocument>) -> Result<PaneId, PaneError> {
        if self.tree.try_with_untracked(PaneTree::is_empty) != Some(true) {
            return Err(PaneError::Layout(tree::TreeError::NotEmpty));
        }
        self.create_placed(launch, |tree, id| tree.set_root(id))
    }

    /// Create a pane and place it, transactionally: a refused placement
    /// closes it again.
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
            // The refused pane's seeded look is forgotten too: ids are never
            // reused.
            self.themes.forget(id);
            let _ = self.manager.close(id, None);
            return Err(PaneError::Layout(error));
        }
        self.relayout_now();
        Ok(id)
    }

    /// Create a pane for `launch` with the current appearance, unplaced.
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
        // A pane born into a live split starts from the pane in front.
        if self.themes.active().get_untracked() || self.themes.textures_active().get_untracked() {
            let global = self.session.settings.with(|s| s.appearance);
            let from = self
                .manager
                .active()
                .filter(|active| *active != id)
                .and_then(|active| self.themes.look_for(active, global))
                .unwrap_or(global);
            let seed = self.themes.distinct_look(from, global);
            self.themes.seed(id, seed);
        }
        if let Some(pane) = self.manager.pane(id) {
            pane.appearance(self.appearance_now(id));
        }
        Ok(id)
    }

    /// The workspace's document placement; see [`OpenTarget`]. A split
    /// refuses cleanly.
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

    /// Open `launch` in pane `id`, in place. A suspended pane is resumed
    /// for it.
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

    /// Close one pane: it leaves the layout first, then the manager
    /// disposes it.
    pub fn close_pane(&self, id: PaneId) -> Result<(), PaneError> {
        match self.manager.lifecycle(id) {
            None => return Err(PaneError::Unknown(id)),
            Some(lifecycle) if !lifecycle.is_live() => return Err(PaneError::Gone(id)),
            Some(_) => {}
        }
        // The survivor's look is read while the split (and so the mode) is
        // still there.
        let survivor = if self.pane_count() == 2 {
            let placed = self.manager.placed();
            placed.into_iter().find(|pane| *pane != id)
        } else {
            None
        };
        let promoted = survivor.and_then(|pane| {
            let global = self.session.settings.get_untracked().appearance;
            self.themes.promote(pane, global)
        });
        let successor = self
            .tree
            .try_update(|tree| tree.remove(id).ok().flatten())
            .flatten();
        let tail = self.manager.close_now(id, successor)?;
        let _ = self.retiring.try_update(|r| r.push(id));
        let retiring = self.retiring;
        let enter = self.session.enter;
        // Unowned: the close disposes the button's owner that would cancel it.
        wasm_bindgen_futures::spawn_local(async move {
            tail.await;
            // The retire cascade belongs in a reactive owner, not this drain.
            enter(&mut move || {
                let _ = retiring.try_update(|r| r.retain(|other| *other != id));
            });
        });
        self.themes.forget(id);
        // The promoted look lands after the close, when both modes stand
        // down.
        if let Some(look) = promoted {
            self.session.settings.update(|s| {
                s.appearance = look;
                s.touch_appearance();
            });
        }
        self.relayout_now();
        Ok(())
    }

    /// The slot's entries: the placed panes and the retiring ones, in id
    /// order.
    pub fn entries(&self) -> Vec<PaneId> {
        let mut ids = self.manager.placed();
        if let Some(retiring) = self.retiring.try_get() {
            for id in retiring {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        ids.sort();
        ids
    }

    /// Whether pane `id` is closed and finishing its teardown.
    pub fn is_retiring(&self, id: PaneId) -> bool {
        self.retiring.try_with(|r| r.contains(&id)).unwrap_or(false)
    }

    /// Move pane `id` one step toward `direction`; only the tree changes.
    pub fn move_pane(&self, id: PaneId, direction: MoveDirection) -> Result<(), PaneError> {
        self.reshape(|tree| tree.move_pane(id, direction))
    }

    /// Exchange two panes' places (a lifted pane dropped on another).
    pub fn swap_panes(&self, a: PaneId, b: PaneId) -> Result<(), PaneError> {
        self.reshape(|tree| tree.swap(a, b))
    }

    /// Re-dock pane `id` on `edge` of pane `target` (a lifted pane dropped
    /// near another's edge).
    pub fn dock_pane(&self, id: PaneId, target: PaneId, edge: Edge) -> Result<(), PaneError> {
        let (axis, side) = edge.placement();
        self.reshape(|tree| tree.dock(id, target, axis, side))
    }

    /// One layout-only change, then every pane is handed its new box.
    fn reshape(
        &self,
        change: impl FnOnce(&mut PaneTree) -> Result<(), tree::TreeError>,
    ) -> Result<(), PaneError> {
        self.tree
            .try_update(change)
            .unwrap_or(Err(tree::TreeError::InvalidMove))
            .map_err(PaneError::Layout)?;
        self.relayout_now();
        Ok(())
    }

    /// The host is dragging (a divider, a document): panes let the
    /// pointer through.
    pub fn shielded(&self) -> bool {
        self.resizing.try_get().unwrap_or(false)
            || self.drag.try_with(DragSession::is_live).unwrap_or(false)
    }

    /// The lifted pane (tracked; `None` while nothing is held).
    pub fn lifted(&self) -> Option<lift::Lift> {
        self.lift.try_get().flatten()
    }

    /// Pick pane `id` up at slot point `at`; only in a split.
    fn begin_lift(&self, id: PaneId, at: (f64, f64)) -> bool {
        let busy = self.drag.try_with_untracked(DragSession::is_live) != Some(false);
        if busy || untrack(|| self.pane_count()) < 2 {
            return false;
        }
        let _ = self.set_active(id);
        self.lift.try_set(Some(lift::Lift::new(id, at))).is_none()
    }

    /// The lifted pane's pointer moved to slot point `at`.
    fn lift_move(&self, at: (f64, f64)) {
        let layout = self.layout_now();
        self.lift.try_update(|lift| {
            if let Some(lift) = lift {
                lift.moved(at, &layout);
            }
        });
    }

    /// Put the lifted pane down: `commit` relocates it, else it returns.
    fn end_lift(&self, commit: bool) {
        let Some(Some(lift)) = self.lift.try_update(Option::take) else {
            return;
        };
        let Some(target) = lift.target.filter(|_| commit) else {
            return;
        };
        let moved = self.in_session(|| match target {
            lift::LiftTarget::Swap(other) => self.swap_panes(lift.pane, other),
            lift::LiftTarget::Dock(other, edge) => self.dock_pane(lift.pane, other, edge),
        });
        if let Some(Err(error)) = moved {
            self.refused(error);
        }
    }

    /// How many panes the workspace shows (tracked).
    pub fn pane_count(&self) -> usize {
        self.manager.placed().len()
    }

    /// Whether some pane holds a PDF document (tracked): the blend
    /// backdrop's workspace-wide gate.
    fn has_pdf(&self) -> bool {
        self.has_pdf.get()
    }

    /// The Library button: panes write their point and stop work, then the
    /// Shell navigates.
    pub fn return_to_library(&self) {
        for pane in self.manager.live_panes() {
            let _ = pane.command(PaneCommand::PrepareLeave);
        }
        self.session.api.navigate_library();
    }

    /// Dispose the workspace: every pane's dispose runs now. Idempotent.
    pub fn dispose(&self) {
        // No drag outlives the workspace: an in-flight one ends here.
        self.drag.try_update_untracked(|session| session.cancel());
        self.lift.try_update_untracked(|lift| *lift = None);
        // Every pane's entry stays in the document, out of sight, until the
        // session unmounts.
        let placed = untrack(|| self.manager.placed());
        let _ = self.retiring.try_update(|r| {
            for id in placed {
                if !r.contains(&id) {
                    r.push(id);
                }
            }
        });
        self.manager.dispose_all();
        // The layout lets go of the panes with the manager; untracked.
        self.tree
            .try_update_untracked(|tree| *tree = PaneTree::new());
    }

    /// The panes' disposal tails, for the session's runtime to await.
    pub fn take_teardown(&self) -> contract::PaneTeardown {
        let shared = self.manager.shared();
        let tails = self.manager.take_teardown();
        // The drag as the dispose left it: proof no drag outlived it.
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
    /// The drag session's lifetime: one window listener set while a drag is
    /// live.
    fn install_drag(&self) {
        let host = *self;
        let live = Memo::new(move |_| host.drag.with(DragSession::is_live));
        Effect::new(move |_| {
            if live.get() {
                install_drag_listeners(host);
            }
        });
        Effect::new(move |_| {
            if host.frame_active.try_get() == Some(false) {
                untrack(|| host.cancel_drag());
            }
        });
    }

    /// The pending drop's preview (tracked; `None` while nothing is aimed).
    pub fn drag_preview(&self) -> Option<drag::Preview> {
        self.drag.try_with(DragSession::preview).flatten()
    }

    /// The address a drag past its threshold carries; the row dims.
    pub fn dragged_path(&self) -> Option<String> {
        self.drag
            .try_with(|session| match session {
                DragSession::Dragging { source, .. } => Some(source.path.clone()),
                _ => None,
            })
            .flatten()
    }

    /// A press on a Library row: arms only; the geometry is measured at
    /// the threshold.
    pub fn library_press(&self, file: &library::LibraryFile, at: (f64, f64)) -> bool {
        let source = DocumentDragSource {
            document: DocumentId::from_launch(Some(&file.book_id), &file.path),
            book_id: Some(file.book_id.clone()),
            path: file.path.clone(),
            format: (self.classify)(&file.path),
            label: file.name.clone(),
        };
        self.drag
            .try_maybe_update(|session| {
                let armed = session.arm(source, at);
                (armed, armed)
            })
            .unwrap_or(false)
    }

    /// The pointer of a live drag moved; subscribers hear only on change.
    fn drag_move(&self, at: (f64, f64)) {
        let host = *self;
        self.drag.try_maybe_update(|session| {
            let changed = session.moved(at, || host.measure_geometry());
            (changed, ())
        });
    }

    /// The pointer was released: the drag ends, a drop over a target runs.
    fn drag_release(&self, at: Option<(f64, f64)>) {
        let released = self.drag.try_maybe_update(|session| {
            let was = session.is_live();
            let dragged = session.is_dragging();
            (was, (dragged, session.release(at)))
        });
        let Some((dragged, intent)) = released else {
            return;
        };
        if dragged {
            self.library.swallow_click();
        }
        if let Some(intent) = intent {
            self.commit_drop(intent);
        }
    }

    /// End the drag with no drop. Returns whether one was live.
    fn cancel_drag(&self) -> bool {
        self.drag
            .try_maybe_update(|session| {
                let was = session.cancel();
                (was, was)
            })
            .unwrap_or(false)
    }

    /// Carry out a drop through the one workspace command.
    fn commit_drop(&self, intent: DropIntent) {
        let DropIntent { source, target } = intent;
        let command = WorkspaceCommand::OpenInDropTarget { source, target };
        if let Some(Err(error)) = self.in_session(|| self.run(command)) {
            self.refused(error);
        }
    }

    /// Run a workspace command: validate now, resolve the source, place
    /// through the one path.
    pub fn run(&self, command: WorkspaceCommand) -> Result<PaneId, PaneError> {
        let WorkspaceCommand::OpenInDropTarget { source, target } = command;
        let layout = self.layout_now();
        let live = untrack(|| self.manager.placed().len());
        let plan = self
            .tree
            .try_with_untracked(|tree| {
                commands::plan(target, tree, &layout, live, |id| self.pane_is_empty(id))
            })
            .unwrap_or(Err(PaneError::HostDisposed))?;
        let launch = launch_for(source.book_id.as_deref(), &source.path, &source.label);
        match plan {
            DropPlan::Split { of, axis, side } => {
                self.open_document(launch, OpenTarget::Split { of, axis, side })
            }
            DropPlan::Here { pane } => self.open_document(launch, OpenTarget::Pane(pane)),
        }
    }

    /// A Library row was activated; `how` says where its document goes.
    pub fn library_open(&self, file: &library::LibraryFile, how: library::OpenHow) {
        let document = DocumentId::from_launch(Some(&file.book_id), &file.path);
        let showing = self
            .manager
            .live_panes()
            .into_iter()
            .find(|pane| document.is_some() && pane.document() == document);
        if let Some(pane) = showing {
            let _ = self.set_active(pane.id());
            return;
        }
        let target = match (how, self.manager.active_untracked()) {
            (library::OpenHow::Beside, Some(of)) if !self.pane_is_empty(of) => OpenTarget::Split {
                of,
                axis: library::beside_axis(self.manager.bounds_untracked(of)),
                side: Side::After,
            },
            _ => OpenTarget::Active,
        };
        let launch = launch_for(Some(&file.book_id), &file.path, &file.name);
        if let Some(Err(error)) = self.in_session(|| self.open_document(launch, target)) {
            self.refused(error);
        }
    }

    /// The placed panes as the Library panel's open-tabs strip lists them.
    pub fn open_tabs(&self) -> Vec<library::OpenTab> {
        self.manager
            .placed()
            .into_iter()
            .filter_map(|id| {
                let pane = self.manager.pane(id)?;
                Some(library::OpenTab {
                    id,
                    name: pane.surface().name,
                })
            })
            .collect()
    }

    /// The rail's Library panel state.
    pub(crate) fn library(&self) -> library::LibraryState {
        self.library
    }

    /// Pane `id` holds no document (the unhosted entry can start empty).
    fn pane_is_empty(&self, id: PaneId) -> bool {
        self.manager
            .pane(id)
            .is_none_or(|pane| pane.document().is_none())
    }

    /// The drag's one measurement: the slot's client rect and the layout.
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

/// The live drag's window listeners, removed by the calling effect's
/// cleanup.
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
    let on_pointer =
        Closure::<dyn Fn(web_sys::PointerEvent)>::new(move |event: web_sys::PointerEvent| {
            let at = (f64::from(event.client_x()), f64::from(event.client_y()));
            match event.type_().as_str() {
                "pointermove" => host.drag_move(at),
                "pointerup" => host.drag_release(Some(at)),
                _ => {
                    host.cancel_drag();
                }
            }
        });
    let _ = window.add_event_listener_with_callback_and_bool(
        "keydown",
        on_key.as_ref().unchecked_ref(),
        true,
    );
    let _ = window.add_event_listener_with_callback("blur", on_blur.as_ref().unchecked_ref());
    for name in POINTER_EVENTS {
        let _ = window.add_event_listener_with_callback(name, on_pointer.as_ref().unchecked_ref());
    }
    let parked = StoredValue::new_local(Some((on_key, on_blur, on_pointer)));
    on_cleanup(move || {
        let Some(Some((on_key, on_blur, on_pointer))) = parked.try_update_value(Option::take)
        else {
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
            for name in POINTER_EVENTS {
                let _ = window
                    .remove_event_listener_with_callback(name, on_pointer.as_ref().unchecked_ref());
            }
        }
    });
}

/// The pointer events a live drag follows at the window.
const POINTER_EVENTS: [&str; 3] = ["pointermove", "pointerup", "pointercancel"];

/// The launch for a library address: the store's resolution, else a
/// bare launch.
fn launch_for(book_id: Option<&str>, path: &str, name: &str) -> LaunchDocument {
    storage::resolve_launch(path).unwrap_or_else(|| LaunchDocument {
        book_id: book_id.map(str::to_string),
        path: path.to_string(),
        resume_page: 1,
        saved_fraction: None,
        blend_override: false,
        cover_data_url: None,
        display_name: Some(name.to_string()),
    })
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
    /// The workspace layout: the split tree over pane ids.
    pub layout: Option<LayoutNode>,
    /// Every pane not yet `Disposed`, in placement order.
    pub panes: Vec<PaneSnapshot>,
    /// Every pane this host ever created.
    pub panes_created: usize,
    /// The document drag session's phase (`idle` when none is live).
    pub drag: &'static str,
}

impl HostSnapshot {
    /// Whether the workspace still reads something: live, with a pane that
    /// is not leaving.
    pub fn still_reading(&self) -> bool {
        still_reading(self.lifecycle, self.panes.iter().map(|pane| pane.lifecycle))
    }
}

fn still_reading(host: &str, mut panes: impl Iterator<Item = PaneLifecycle>) -> bool {
    host == "live"
        && panes.any(|pane| !matches!(pane, PaneLifecycle::Disposing | PaneLifecycle::Disposed))
}

#[cfg(test)]
mod pane_bounds_tests {
    use super::inset_pane_bounds;
    use crate::host::model::PaneBounds;

    #[test]
    fn pane_spacing_covers_the_outer_edges_and_split_gutter() {
        let workspace = PaneBounds {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 500.0,
        };
        assert_eq!(
            inset_pane_bounds(
                PaneBounds {
                    x: 0.0,
                    y: 0.0,
                    width: 500.0,
                    height: 500.0
                },
                workspace,
                12,
            ),
            PaneBounds {
                x: 12.0,
                y: 12.0,
                width: 482.0,
                height: 476.0
            },
        );
        assert_eq!(
            inset_pane_bounds(
                PaneBounds {
                    x: 500.0,
                    y: 0.0,
                    width: 500.0,
                    height: 500.0
                },
                workspace,
                12,
            ),
            PaneBounds {
                x: 506.0,
                y: 12.0,
                width: 482.0,
                height: 476.0
            },
        );
    }

    #[test]
    fn a_gutter_never_makes_a_narrow_pane_negative() {
        let bounds = PaneBounds {
            x: 4.0,
            y: 8.0,
            width: 8.0,
            height: 10.0,
        };
        let workspace = PaneBounds {
            x: 0.0,
            y: 8.0,
            width: 20.0,
            height: 10.0,
        };
        assert_eq!(
            inset_pane_bounds(bounds, workspace, 24),
            PaneBounds {
                x: 8.0,
                y: 13.0,
                width: 0.0,
                height: 0.0
            },
        );
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::{PaneLifecycle, still_reading};

    #[test]
    fn a_pane_closing_beside_a_live_one_is_not_the_reader_draining() {
        use PaneLifecycle::{Disposed, Disposing, Ready, Suspended};
        assert!(still_reading("live", [Disposing, Ready].into_iter()));
        assert!(still_reading("live", [Suspended].into_iter()));
        assert!(!still_reading("live", [Disposing].into_iter()));
        assert!(!still_reading("live", [Disposed, Disposing].into_iter()));
        assert!(!still_reading("live", std::iter::empty()));
        assert!(!still_reading("disposed", [Ready].into_iter()));
    }
}

/// One pane in the snapshot.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneSnapshot {
    pub pane_id: PaneId,
    pub document_id: Option<DocumentId>,
    pub format: PaneFormat,
    pub lifecycle: PaneLifecycle,
    /// The one active pane (inactive is not disposed, only unfocused).
    pub focused: bool,
    pub bounds: PaneBounds,
    /// The pane's box in CSS px², what its raster demand scales with.
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
            // A frame pane mirrors its document over the port, which can trail
            // by a message.
            document_id: runtime.and_then(|pane| pane.document()).or_else(|| {
                record
                    .descriptor
                    .document
                    .as_ref()
                    .map(|d| d.document_id.clone())
            }),
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
