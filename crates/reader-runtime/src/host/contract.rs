//! The pane runtime contract: the one interface between the reader host and
//! whatever renders a document inside a pane.
//!
//! Dependency direction: `host → contract → format`. The host holds panes
//! only as `Rc<dyn PaneRuntime>` and speaks to them only through this trait
//! and the data types beside it; the format implementation (today the
//! universal document pane in `crate::pane`, which serves PDF, Markdown and
//! plain text through one pipeline) implements it. Nothing in this file, and
//! nothing in `crate::host`, may name a PDF, reflow or engine type —
//! `tools/check-host-boundary.mjs` enforces that in CI.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use leptos::prelude::*;
use runtime_contract::boundary::LaunchDocument;

use super::model::{
    DocumentId, PaneBounds, PaneDescriptor, PaneError, PaneFormat, PaneId, PaneLifecycle,
};

/// A document status in words the host may know: no format, no engine.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PaneDocStatus {
    #[default]
    Idle,
    Opening,
    Ready,
    Error,
}

impl PaneDocStatus {
    /// The word the Shell's status report carries (the boundary's
    /// `DocStatusReport::status`).
    pub fn word(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Opening => "Opening",
            Self::Ready => "Ready",
            Self::Error => "Error",
        }
    }

    /// A document is open or on its way: what the Library button and the
    /// Shell's route policy ask.
    pub fn holds_document(self) -> bool {
        matches!(self, Self::Opening | Self::Ready)
    }
}

/// The format-neutral facts a pane publishes for the host's chrome and its
/// reports. Read-only signals owned by the pane: they die with the pane's
/// reactive owner, so every host-side read goes through `try_`.
#[derive(Clone, Copy)]
pub struct PaneSurface {
    pub status: Signal<PaneDocStatus>,
    pub error: Signal<Option<String>>,
    /// The 1-based page the pane's viewport is on.
    pub page: Signal<u32>,
    /// The chrome's one yes/no about the document's kind (the appearance
    /// menu's paper-only sections, the backdrop's blend class).
    pub reflowable: Signal<bool>,
    /// The pane's find bar is open (the title bar holds itself up for it).
    pub search_visible: Signal<bool>,
}

/// The chrome regions the HOST places and a pane may fill for its document.
/// The host owns where each region sits and when it is shown; the active
/// pane owns what is inside it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChromeSlot {
    /// The title bar's centre: the document title.
    TitleCenter,
    /// The title bar's trailing cluster, before the host's own appearance
    /// menu: the document's view menu.
    TitleTrailing,
    /// The sidebar rail's content (the host owns both mount points and the
    /// open/close machine).
    Rail,
    /// The settings modal's content, shown while the host's settings-open
    /// signal is up.
    Settings,
}

/// What the host's appearance boundary hands a pane: the resolved look the
/// pane must honour, pushed on every change. Panes never read the host's
/// settings to derive these themselves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaneAppearance {
    pub motion: app_state::Motion,
}

/// Where the host is placing a pane's view: the shared-chrome context
/// visible at the placement site, handed over explicitly because a pane's
/// views are owned by the PANE's reactive owner, whose ancestry is the
/// host's owner rather than the title bar the view sits in. Format-neutral
/// chrome types only.
#[derive(Clone, Copy, Default)]
pub struct PaneSite {
    /// The title bar's shared state (its visibility, the popover holds, the
    /// centre-title node), when the placement sits inside the title bar's
    /// tree.
    pub title_bar: Option<app_chrome::titlebar::root::TitleBarCtx>,
}

impl PaneSite {
    /// The site of the CURRENT reactive scope: call it inside the host's
    /// placement closure.
    pub fn here() -> Self {
        Self {
            title_bar: use_context::<app_chrome::titlebar::root::TitleBarCtx>(),
        }
    }
}

/// Workspace commands the host routes to a pane.
#[derive(Clone, Debug)]
pub enum PaneCommand {
    /// Open this launch in the pane, replacing its current document (the
    /// pane keeps its id; its document session is replaced).
    Open(Box<LaunchDocument>),
    /// The workspace is about to leave (Library): write the durable reading
    /// point and stop in-flight raster work now, before the Shell's dispose
    /// comes back over the boundary.
    PrepareLeave,
}

/// The async half of a pane's disposal (the engine's awaited destroy, the
/// virtualizers' final dispose). The sync half — owner cleanup, listener,
/// observer and timer release — has already run when this is returned.
///
/// The future owns exactly what it still has to release and NEVER the pane
/// runtime itself: the manager drops the pane object the moment `dispose`
/// returns, so nothing but the tail's own captures outlives the sync half.
pub type PaneTeardown = Pin<Box<dyn Future<Output = ()>>>;

/// A pane's live resources, as the pane itself counts them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneResourceCounts {
    pub virtualizers: usize,
    /// Whether the pane holds an open document session.
    pub document_session: bool,
}

/// One pane runtime. The host calls these and nothing else.
///
/// Call discipline (the manager guarantees it): `mount` once, after the
/// manager moved the pane to `Mounting`; `resize`/`focus`/`blur`/
/// `appearance`/`command` only while the pane is live; `dispose` exactly
/// once, after the manager moved the pane to `Disposing`.
pub trait PaneRuntime {
    fn id(&self) -> PaneId;

    /// The format tag of the pane's CURRENT document, for labels only (the
    /// host never branches on it).
    fn format(&self) -> PaneFormat;

    /// The identity of the pane's CURRENT document (an in-place open changes
    /// it; the pane id stays).
    fn document(&self) -> Option<DocumentId>;

    /// The manager's publication of a lifecycle transition its core made.
    /// The pane mirrors it for its own work gates — a copy of the core's
    /// answer, never a second authority. The manager publishes up to
    /// `Disposing`; the pane's own teardown tail closes its gates as
    /// `Disposed` when it finishes (the manager holds no pane by then).
    fn lifecycle_changed(&self, lifecycle: PaneLifecycle);

    /// The pane's format-neutral published facts.
    fn surface(&self) -> PaneSurface;

    /// Build the pane's content for the host's workspace slot, sized to
    /// `bounds`. The view is owned by a child of the pane's reactive owner:
    /// it dies when the host drops it or when the pane is disposed,
    /// whichever comes first.
    fn mount(&self, bounds: PaneBounds, site: PaneSite) -> AnyView;

    /// The pane's contribution to one host-placed chrome region, if any.
    /// Owned like `mount`'s view.
    fn chrome(&self, slot: ChromeSlot, site: PaneSite) -> Option<AnyView>;

    /// The host measured new bounds for this pane.
    fn resize(&self, bounds: PaneBounds);

    /// The focus authority made this pane active.
    fn focus(&self);

    /// The focus authority moved away from this pane.
    fn blur(&self);

    /// The host's appearance boundary changed.
    fn appearance(&self, appearance: PaneAppearance);

    /// A workspace command routed to this pane.
    fn command(&self, command: PaneCommand) -> Result<(), PaneError>;

    /// The pane's own count of what it holds.
    fn resources(&self) -> PaneResourceCounts;

    /// Release everything the pane owns: its document session, its render
    /// and prefetch work, its virtualizers, its listeners, observers and
    /// timers, its reactive owner. The sync half runs now; the returned
    /// future is the awaited tail (see [`PaneTeardown`] for what it may
    /// hold). This is the last call the manager makes on the pane.
    fn dispose(&self) -> PaneTeardown;
}

/// Builds the pane runtime for a descriptor. The composition root (the
/// session start in `crate`) injects the format implementation here, so the
/// host itself never names one.
pub type PaneFactory = Rc<PaneBuild>;

/// The factory's signature: the pane's environment, its descriptor, and the
/// launch it opens first (none for a pane waiting for one).
pub type PaneBuild = dyn Fn(PaneEnv, PaneDescriptor, Option<LaunchDocument>) -> Rc<dyn PaneRuntime>;

/// Names the format tag of a document address, for the descriptor the host
/// records BEFORE the pane exists. Injected beside the factory by the same
/// composition root: which extensions mean which format is the pane
/// implementation's knowledge, never the host's.
pub type PaneClassifier = fn(&str) -> PaneFormat;

/// What the host hands every pane it creates: Copy handles onto the
/// session's own slices — never the Shell's state, never another pane's.
#[derive(Clone, Copy)]
pub struct PaneEnv {
    pub runtime: crate::runtime::ReaderRuntime,
    pub settings: RwSignal<reader_core::settings::Settings>,
    pub ui: app_state::UiState,
    pub api: crate::context::ApiHandle,
    pub session_id: u32,
    pub chrome: app_state::ChromeState,
    /// This pane is the host's active pane — DERIVED from the host's one
    /// focus authority, never a second store of it.
    pub active: Signal<bool>,
    /// The settings-open signal the host's modal slot follows.
    pub settings_open: RwSignal<bool>,
    /// Ask the host's focus authority to make THIS pane active (a pointer
    /// or keyboard focus landing inside it). A request: the authority
    /// decides, and the pane hears the answer through `focus`/`blur` and
    /// `active`.
    pub request_focus: Callback<()>,
    /// The host's workspace open command: a document the pane's user picks
    /// is handed back to the host, which routes it (to its active pane).
    pub open: Callback<LaunchDocument>,
}
