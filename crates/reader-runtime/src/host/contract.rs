//! The host ↔ pane contract; no format or engine type here.

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

    /// A document is open or on its way.
    pub fn holds_document(self) -> bool {
        matches!(self, Self::Opening | Self::Ready)
    }
}

/// The format-neutral facts a pane publishes; the pane's signals die
/// with it.
#[derive(Clone, Copy)]
pub struct PaneSurface {
    pub status: Signal<PaneDocStatus>,
    pub error: Signal<Option<String>>,
    /// The 1-based page the pane's viewport is on.
    pub page: Signal<u32>,
    /// The chrome's one yes/no about the document's kind.
    pub reflowable: Signal<bool>,
    /// The pane's find bar is open (the title bar holds itself up for it).
    pub search_visible: Signal<bool>,
    /// What the document is called.
    pub name: Signal<String>,
}

/// The chrome regions the host places; the active pane fills them.
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

/// The resolved look the host pushes on every change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaneAppearance {
    pub motion: app_state::Motion,
    /// The pane's own look while a per-pane mode is in effect; `None`
    /// inherits the window's.
    pub look: Option<reader_core::appearance::Appearance>,
}

/// The shared-chrome context at the placement site.
#[derive(Clone, Copy, Default)]
pub struct PaneSite {
    /// The title bar's shared state, when the site sits in its tree.
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
    /// Open this launch in the pane, replacing its document.
    Open(Box<LaunchDocument>),
    /// The workspace is about to leave: write the read point now.
    PrepareLeave,
}

/// The async half of a pane's disposal; the sync half has already
/// run.
pub type PaneTeardown = Pin<Box<dyn Future<Output = ()>>>;

/// A pane's live resources, as the pane itself counts them.
#[derive(Clone, Copy, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneResourceCounts {
    pub virtualizers: usize,
    /// Whether the pane holds an open document session.
    pub document_session: bool,
    /// The scale the pane displays its document at.
    pub zoom: f64,
}

/// One pane runtime. The host calls these and nothing else.
pub trait PaneRuntime {
    fn id(&self) -> PaneId;

    /// The format tag of the pane's CURRENT document, for labels only.
    fn format(&self) -> PaneFormat;

    /// The identity of the pane's CURRENT document.
    fn document(&self) -> Option<DocumentId>;

    /// The manager's publication of a transition; the pane mirrors it.
    fn lifecycle_changed(&self, lifecycle: PaneLifecycle);

    /// The pane's format-neutral published facts.
    fn surface(&self) -> PaneSurface;

    /// Build the pane's content for the host's workspace slot.
    fn mount(&self, bounds: PaneBounds, site: PaneSite) -> AnyView;

    /// The pane's contribution to one host-placed chrome region, if any.
    fn chrome(&self, _slot: ChromeSlot, _site: PaneSite) -> Option<AnyView> {
        None
    }

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

    /// Release everything the pane owns; the returned future is the
    /// awaited tail.
    fn dispose(&self) -> PaneTeardown;
}

/// Builds the pane runtime for a descriptor; the composition root
/// injects the format.
pub type PaneFactory = Rc<PaneBuild>;

/// The factory's signature.
pub type PaneBuild = dyn Fn(PaneEnv, PaneDescriptor, Option<LaunchDocument>) -> Rc<dyn PaneRuntime>;

/// Names the format tag of a document address, before the pane
/// exists.
pub type PaneClassifier = fn(&str) -> PaneFormat;

/// What the host hands every pane it creates.
#[derive(Clone, Copy)]
pub struct PaneEnv {
    pub runtime: crate::runtime::ReaderRuntime,
    pub settings: RwSignal<reader_core::settings::Settings>,
    pub ui: app_state::UiState,
    pub api: crate::context::ApiHandle,
    pub session_id: u32,
    pub chrome: app_state::ChromeState,
    /// This pane is the host's active pane; derived from one authority.
    pub active: Signal<bool>,
    /// The settings-open signal the host's modal slot follows.
    pub settings_open: RwSignal<bool>,
    /// Ask the focus authority to make THIS pane active.
    pub request_focus: Callback<()>,
    /// The host's workspace open command.
    pub open: Callback<OpenRequest>,
    /// Whether the workspace would take another pane now.
    pub can_split: Signal<bool>,
    /// Which ways the layout could move this pane now.
    pub moves: Signal<super::tree::Moves>,
    /// Ask the host to move THIS pane one step through the layout.
    pub relocate: Callback<super::tree::MoveDirection>,
    /// The workspace facts a pane's own document mirrors.
    pub workspace: Signal<WorkspaceLook>,
    /// A press-and-hold lift that began inside the pane, in the host
    /// document's client coordinates.
    pub lift: Callback<LiftStep>,
}

/// The workspace facts the shared CSS keys off.
#[derive(Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceLook {
    pub blend: bool,
    /// Independent themes in effect: the preference while a split is on
    /// screen.
    pub independent: bool,
    pub split: bool,
    pub page_shadow: bool,
    /// The `--pane-*` custom properties, as one inline style string.
    pub style: String,
}

/// A phase of a pane lift driven from inside the pane.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiftPhase {
    Start,
    Move,
    End,
    Cancel,
}

/// One lift step: the phase and the pointer in host client coordinates.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LiftStep {
    pub phase: LiftPhase,
    pub at: (f64, f64),
}

/// Where a pane asks the host to put a document, relative to ITSELF.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Placement {
    /// In the asking pane, in place: its document session is replaced.
    Here,
    /// In a new pane beside the asking one, split along the axis.
    Beside(super::tree::SplitAxis),
}

/// A pane's open request: the document and where it goes.
#[derive(Clone, PartialEq, Debug)]
pub struct OpenRequest {
    pub launch: LaunchDocument,
    pub placement: Placement,
}
