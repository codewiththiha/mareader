//! The vocabulary a workspace host and its pane frames speak over the pane's
//! `MessageChannel` (docs/pane-runtimes.md). Both ends are this crate: the
//! host half is `crate::frame_pane`, the pane half `crate::pane_frame`.
//!
//! Every message but one is a JSON string of [`HostToPane`] or
//! [`PaneToHost`]. The exception is a rendered thumbnail, which travels as a
//! plain object carrying a transferred `ImageBitmap` (see
//! [`THUMB_MESSAGE`]): pixels are not JSON.

use pdf_engine::types::{DocStatus, PageSize};
use reader_core::appearance::Appearance;
use reader_core::format::Format;
use reader_core::settings::Settings;
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;
use runtime_contract::boundary::LaunchDocument;
use serde::{Deserialize, Serialize};

use crate::host::contract::PaneResourceCounts;
pub use crate::host::contract::{LiftPhase, WorkspaceLook as Workspace};
use crate::host::model::{PaneFormat, PaneLifecycle};
use crate::host::tree::{MoveDirection, Moves, SplitAxis};

/// The `kind` of the one window message that is not on the port: the host's
/// `postMessage` that hands a freshly loaded pane frame its port.
pub const PANE_CHANNEL_KIND: &str = "mareader.pane";

/// The `kind` of a pane frame's one message to its parent window: "my
/// document is up, hand me my port", with the nonce from the frame's URL.
pub const PANE_HELLO_KIND: &str = "mareader.pane.hello";

/// The `t` of the object message that carries a thumbnail bitmap.
pub const THUMB_MESSAGE: &str = "thumb";

/// Which pane runtime a frame runs: the page it loads and the wasm behind it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneKind {
    Pdf,
    Reflow,
}

impl PaneKind {
    /// The runtime a format opens in. A pane with no document yet waits in
    /// the reflow runtime: it never loads pdf.js, and a PDF open replaces it.
    pub fn for_format(format: PaneFormat) -> Self {
        match format {
            PaneFormat::Pdf => Self::Pdf,
            _ => Self::Reflow,
        }
    }

    /// The runtime a path opens in, by its extension.
    pub fn for_path(path: &str) -> Self {
        Self::for_format(crate::pane::document::classify(path))
    }

    /// The page the frame loads.
    pub fn page(self) -> &'static str {
        match self {
            Self::Pdf => "pdf.html",
            Self::Reflow => "reflow.html",
        }
    }
}

/// The chrome motion profile, in a shape that crosses the port.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct WireMotion {
    pub sidebar_slide: bool,
    pub canvas_resize: bool,
    pub zoom: bool,
    pub scroll_glide: bool,
}

impl From<app_state::Motion> for WireMotion {
    fn from(m: app_state::Motion) -> Self {
        Self {
            sidebar_slide: m.sidebar_slide,
            canvas_resize: m.canvas_resize,
            zoom: m.zoom,
            scroll_glide: m.scroll_glide,
        }
    }
}

impl From<WireMotion> for app_state::Motion {
    fn from(m: WireMotion) -> Self {
        Self {
            sidebar_slide: m.sidebar_slide,
            canvas_resize: m.canvas_resize,
            zoom: m.zoom,
            scroll_glide: m.scroll_glide,
        }
    }
}

/// The open rail panel, in a shape that crosses the port.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireSidebar {
    None,
    Outline,
    Thumbs,
    Library,
}

impl From<app_state::SidebarMode> for WireSidebar {
    fn from(m: app_state::SidebarMode) -> Self {
        match m {
            app_state::SidebarMode::None => Self::None,
            app_state::SidebarMode::Outline => Self::Outline,
            app_state::SidebarMode::Thumbs => Self::Thumbs,
            app_state::SidebarMode::Library => Self::Library,
        }
    }
}

impl From<WireSidebar> for app_state::SidebarMode {
    fn from(m: WireSidebar) -> Self {
        match m {
            WireSidebar::None => Self::None,
            WireSidebar::Outline => Self::Outline,
            WireSidebar::Thumbs => Self::Thumbs,
            WireSidebar::Library => Self::Library,
        }
    }
}

/// The paper every pane shows in ordinary blend: the most recently focused
/// PDF pane's engine colours.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Paper {
    pub raw: String,
    pub baked: String,
}

/// Everything a pane frame needs to build its pane.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Boot {
    pub pane_id: u64,
    pub session_id: u32,
    pub format: PaneFormat,
    pub launch: Option<LaunchDocument>,
    pub initial_page: u32,
    pub initial_zoom: Option<f64>,
    pub settings: Settings,
    pub motion: WireMotion,
    pub look: Option<Appearance>,
    pub workspace: Workspace,
    pub paper: Option<Paper>,
    pub active: bool,
    pub can_split: bool,
    pub moves: Moves,
    pub sidebar: WireSidebar,
    pub settings_open: bool,
}

/// The chrome-facing state of a pane, sent whole whenever any of it changes.
/// The host's mirror `ReaderState` is written from it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Mirror {
    pub status: DocStatus,
    pub format: Format,
    pub error: Option<String>,
    pub path: Option<String>,
    pub book_id: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub num_pages: u32,
    pub outline_pending: bool,
    pub page1: Option<PageSize>,
    pub page: u32,
    pub mode: ViewMode,
    pub fit: FitMode,
    pub zoom: f64,
    pub auto_scroll: bool,
    pub search_visible: bool,
    pub first_paint: bool,
    pub launch: LaunchDocument,
    pub resources: PaneResourceCounts,
}

/// One outline entry: title, page, depth.
pub type WireOutline = Vec<(String, u32, u32)>;

/// A chrome write the host forwards to the pane that owns the state.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "w", rename_all = "snake_case")]
pub enum Write {
    Page { page: u32 },
    Mode { mode: ViewMode },
    Fit { fit: FitMode },
    ZoomStep { step: i32 },
    AutoScroll { on: bool },
    SearchVisible { on: bool },
}

/// The appearance engine hooks the host's appearance menu drives.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "h", rename_all = "snake_case")]
pub enum Hook {
    Refresh,
    Scrub { on: bool },
    MenuOpen { on: bool },
}

/// Where a pane asks the host to open a document.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WirePlacement {
    Here,
    Beside(SplitAxis),
}

impl From<crate::host::contract::Placement> for WirePlacement {
    fn from(p: crate::host::contract::Placement) -> Self {
        match p {
            crate::host::contract::Placement::Here => Self::Here,
            crate::host::contract::Placement::Beside(axis) => Self::Beside(axis),
        }
    }
}

impl From<WirePlacement> for crate::host::contract::Placement {
    fn from(p: WirePlacement) -> Self {
        match p {
            WirePlacement::Here => Self::Here,
            WirePlacement::Beside(axis) => Self::Beside(axis),
        }
    }
}

/// A key event the host document received while the pane is active.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Key {
    /// `keyup` rather than `keydown`.
    pub up: bool,
    pub repeat: bool,
    pub key: String,
    pub code: String,
    pub meta: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum HostToPane {
    Boot(Box<Boot>),
    Settings(Box<Settings>),
    Appearance {
        motion: WireMotion,
        look: Option<Appearance>,
    },
    Workspace(Workspace),
    Paper(Paper),
    Active {
        on: bool,
    },
    Layout {
        can_split: bool,
        moves: Moves,
    },
    Sidebar {
        mode: WireSidebar,
    },
    SettingsOpen {
        on: bool,
    },
    Lifecycle {
        lifecycle: PaneLifecycle,
    },
    Write(Write),
    Hook(Hook),
    Open(Box<LaunchDocument>),
    PrepareLeave,
    /// Render page `page`'s thumbnail and answer with a bitmap tagged `req`.
    Thumb {
        req: u64,
        page: u32,
    },
    ThumbCancel {
        req: u64,
    },
    /// Warm the thumbnail cache around `page`.
    ThumbPrefetch {
        page: u32,
    },
    /// A key pressed while the host document had focus, for the active pane.
    Key(Key),
    Dispose,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum PaneToHost {
    /// A press inside the pane: it takes focus, and the host's own
    /// outside-press handlers (menus, popovers) see it.
    Press,
    /// The pane's first frame is on screen.
    Painted,
    Mirror(Box<Mirror>),
    Outline {
        entries: WireOutline,
    },
    /// The pane's own shell call (a read point, a cover, a digest…), as the
    /// JSON of a `runtime_contract::protocol::RuntimeEnvelope`.
    Api {
        envelope: String,
    },
    /// A settings change made inside the pane.
    Settings(Box<Settings>),
    Focus,
    Open {
        launch: Box<LaunchDocument>,
        placement: WirePlacement,
    },
    /// The pane's open dialog picked a path the host resolves into a launch.
    OpenPath {
        path: String,
        placement: WirePlacement,
    },
    Relocate {
        direction: MoveDirection,
    },
    Sidebar {
        mode: WireSidebar,
    },
    SettingsOpen {
        on: bool,
    },
    /// The engine's paper colours on the pane's root.
    Paper(Paper),
    ThumbFailed {
        req: u64,
        cancelled: bool,
    },
    Lift {
        phase: LiftPhase,
        x: f64,
        y: f64,
    },
    Disposed,
}

/// Encode a message for the port.
pub fn encode<T: Serialize>(message: &T) -> Option<String> {
    serde_json::to_string(message).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_messages_round_trip() {
        let messages = vec![
            HostToPane::Active { on: true },
            HostToPane::Write(Write::ZoomStep { step: -1 }),
            HostToPane::Write(Write::Mode {
                mode: ViewMode::Spread,
            }),
            HostToPane::Hook(Hook::Scrub { on: true }),
            HostToPane::Layout {
                can_split: true,
                moves: Moves {
                    left: true,
                    ..Moves::default()
                },
            },
            HostToPane::Thumb { req: 7, page: 3 },
            HostToPane::Dispose,
        ];
        for message in messages {
            let json = encode(&message).expect("encodes");
            let back: HostToPane = serde_json::from_str(&json).expect("decodes");
            assert_eq!(back, message);
        }
    }

    #[test]
    fn pane_messages_round_trip() {
        let messages = vec![
            PaneToHost::Press,
            PaneToHost::Focus,
            PaneToHost::Relocate {
                direction: MoveDirection::Left,
            },
            PaneToHost::Paper(Paper {
                raw: "#fff".into(),
                baked: "#eee".into(),
            }),
            PaneToHost::OpenPath {
                path: "/a.md".into(),
                placement: WirePlacement::Beside(SplitAxis::Vertical),
            },
            PaneToHost::Lift {
                phase: LiftPhase::Move,
                x: 1.0,
                y: 2.0,
            },
            PaneToHost::Disposed,
        ];
        for message in messages {
            let json = encode(&message).expect("encodes");
            let back: PaneToHost = serde_json::from_str(&json).expect("decodes");
            assert_eq!(back, message);
        }
    }

    #[test]
    fn a_document_less_pane_waits_in_the_reflow_runtime() {
        assert_eq!(PaneKind::for_format(PaneFormat::Pending), PaneKind::Reflow);
        assert_eq!(PaneKind::for_format(PaneFormat::Pdf), PaneKind::Pdf);
        assert_eq!(PaneKind::for_path("/b/x.md"), PaneKind::Reflow);
        assert_eq!(PaneKind::for_path("/b/x.pdf"), PaneKind::Pdf);
    }
}
