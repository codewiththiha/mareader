//! Inline SVG icon sprite (lucide-style strokes), rendered via inner_html so
//! nothing ever touches the svg element nodes.

use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconName {
    Open,
    ZoomIn,
    ZoomOut,
    FitWidth,
    FitPage,
    Prev,
    Next,
    Outline,
    Search,
    Thumbs,
    Sun,
    Moon,
    Dim,
    Plus,
    Close,
    Check,
    SinglePage,
    DualPage,
    Continuous,
    HScroll,
    /// The 3-dash menu trigger (Readest-style).
    Dashes,
    AutoScroll,
    Sidebar,
    /// Filled rail — used on the close-sidebar button so it reads as "on".
    SidebarOpen,
    ChevronUp,
    ChevronDown,
    Palette,
    More,
    Keyboard,
    Pin,
    // Back-to-library (the recent-books shelf).
    Library,
    // Drag-and-drop feedback overlay.
    Drop,
    /// Counter-clockwise arrow — the undo affordance on toasts.
    Undo,
    /// A clockwise circle-arrow — the "Reload Window" row in the reader's and
    /// the shelf's menus: the app's own restart, and the honest reset for a
    /// footprint that latched. Its own glyph rather than [`IconName::Undo`]
    /// borrowed and flipped, because the two actions are not cousins: an undo
    /// takes one step back, a reload starts the session over.
    Reload,
    /// Two chain links — a library row that points at a book rather than being
    /// one. Its own glyph rather than a borrowed arrow, because the row it sits
    /// on is the one thing on a shelf that is not a file.
    Link,
    /// Two sheets, one behind the other — the shelf's "Duplicate": a second
    /// instance of the row under the pointer. A copy of a document rather than
    /// a folder or a link, because what it makes is one more book.
    Copy,
    /// A plain folder — the "Reveal in folder" row: the OS's own file manager,
    /// opened on the item inside the directory it lives in. `Open`'s folder
    /// wears a line across it because that one opens a book; this one is the
    /// directory itself, and the two rows sit in the same menu.
    Folder,
    /// An open eye — the row that turns a folder's watch ON: the library looking
    /// at a directory, which is the whole of what watching one is. A pair with
    /// [`IconName::EyeOff`] rather than one glyph on a flipping label, because
    /// the row names the action and not the state, and the two actions are
    /// opposites.
    Eye,
    /// The same eye, struck through — the row that turns a folder's watch OFF.
    EyeOff,
    /// A pencil — the row that renames the thing under the pointer: a name is
    /// written rather than a document edited, which is the whole of what the
    /// row does.
    Pencil,
    Settings,
    Layout,
    /// The settings tab for the reflowable formats' typography.
    Type,
    /// The settings tab for motion: a rail that eases, a page that follows.
    Motion,
    Minus,
    // The frameless caption glyphs (Windows/Linux titlebar). Their own family
    // on purpose: the minimize glyph is `Minus`'s twin but lives in window
    // chrome, so swapping one must never silently re-skin the other.
    WindowMinimize,
    WindowMaximize,
    WindowRestore,
}

fn icon_data(name: IconName) -> (&'static str, &'static str) {
    // (viewBox, inner SVG markup)
    match name {
        IconName::Open => ("0 0 24 24", "<path d='M2 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2z'/><path d='M2 10h20'/>"),
        IconName::ZoomIn => ("0 0 24 24", "<circle cx='11' cy='11' r='7'/><path d='m21 21-4.3-4.3'/><path d='M11 8v6M8 11h6'/>"),
        IconName::ZoomOut => ("0 0 24 24", "<circle cx='11' cy='11' r='7'/><path d='m21 21-4.3-4.3'/><path d='M8 11h6'/>"),
        IconName::FitWidth => ("0 0 24 24", "<path d='M3 5v14M21 5v14'/><path d='M8 9l-3 3 3 3M16 9l3 3-3 3'/>"),
        IconName::FitPage => ("0 0 24 24", "<rect x='4' y='4' width='16' height='16' rx='2'/><path d='m9 9 6 6M9 15l6-6'/>"),
        IconName::Prev => ("0 0 24 24", "<path d='m15 18-6-6 6-6'/>"),
        IconName::Next => ("0 0 24 24", "<path d='m9 18 6-6-6-6'/>"),
        IconName::Outline => ("0 0 24 24", "<path d='M8 6h13M8 12h13M8 18h13'/><path d='M3 6h.01M3 12h.01M3 18h.01'/>"),
        IconName::Search => ("0 0 24 24", "<circle cx='11' cy='11' r='7'/><path d='m21 21-4.3-4.3'/>"),
        IconName::Thumbs => ("0 0 24 24", "<rect x='3' y='3' width='7' height='7' rx='1'/><rect x='14' y='3' width='7' height='7' rx='1'/><rect x='3' y='14' width='7' height='7' rx='1'/><rect x='14' y='14' width='7' height='7' rx='1'/>"),
        IconName::Sun => ("0 0 24 24", "<circle cx='12' cy='12' r='4'/><path d='M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4'/>"),
        IconName::Moon => ("0 0 24 24", "<path d='M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z'/>"),
        IconName::Dim => ("0 0 24 24", "<circle cx='12' cy='12' r='9'/><path d='M12 3v18'/>"),
        IconName::Plus => ("0 0 24 24", "<path d='M12 5v14M5 12h14'/>"),
        IconName::Close => ("0 0 24 24", "<path d='M18 6 6 18M6 6l12 12'/>"),
        IconName::Link => ("0 0 24 24", "<path d='M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71'/><path d='M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71'/>"),
        IconName::Copy => ("0 0 24 24", "<rect x='8' y='8' width='14' height='14' rx='2'/><path d='M4 16a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2'/>"),
        IconName::Folder => ("0 0 24 24", "<path d='M2 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2z'/>"),
        IconName::Eye => ("0 0 24 24", "<path d='M2.06 12.35a1 1 0 0 1 0-.7 10.75 10.75 0 0 1 19.88 0 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-19.88 0'/><circle cx='12' cy='12' r='3'/>"),
        IconName::EyeOff => ("0 0 24 24", "<path d='M10.73 5.08A10.75 10.75 0 0 1 12 5a10.75 10.75 0 0 1 9.94 6.65 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-4.43 5.14'/><path d='M6.61 6.61A10.75 10.75 0 0 0 2.06 11.65a1 1 0 0 0 0 .7 10.75 10.75 0 0 0 15.31 5.04'/><path d='M14.12 14.12a3 3 0 1 1-4.24-4.24'/><path d='m2 2 20 20'/>"),
        IconName::Check => ("0 0 24 24", "<path d='M20 6 9 17l-5-5'/>"),
        IconName::Pencil => ("0 0 24 24", "<path d='M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z'/>"),
        IconName::SinglePage => ("0 0 24 24", "<rect x='4' y='3' width='16' height='18' rx='2'/><path d='M4 9h16'/>"),
        IconName::DualPage => ("0 0 24 24", "<rect x='3' y='4' width='8.5' height='16' rx='1.5'/><rect x='12.5' y='4' width='8.5' height='16' rx='1.5'/>"),
        IconName::Continuous => ("0 0 24 24", "<rect x='4' y='3' width='16' height='4' rx='1'/><rect x='4' y='10' width='16' height='4' rx='1'/><rect x='4' y='17' width='16' height='4' rx='1'/>"),
        IconName::HScroll => ("0 0 24 24", "<rect x='2.5' y='6' width='5.5' height='12' rx='1'/><rect x='9.25' y='6' width='5.5' height='12' rx='1'/><rect x='16' y='6' width='5.5' height='12' rx='1'/>"),
        IconName::Dashes => ("0 0 24 24", "<path d='M4 7h16'/><path d='M4 12h16'/><path d='M4 17h16'/>"),
        IconName::AutoScroll => ("0 0 24 24", "<path d='M12 5v14'/><path d='m6 13 6 6 6-6'/>"),
        IconName::Sidebar => ("0 0 24 24", "<rect x='3' y='4' width='18' height='16' rx='2'/><path d='M9 4v16'/>"),
        IconName::SidebarOpen => ("0 0 24 24", "<rect x='3' y='4' width='18' height='16' rx='2'/><path d='M9 4v16'/><path d='M4.5 5.5H9v13H4.5z' fill='currentColor' stroke='none'/>"),
        IconName::Palette => ("0 0 24 24", "<path d='M12 22a10 10 0 1 1 10-10c0 2-1.5 3-3 3h-2a2 2 0 0 0-2 2c0 1 .5 1.5 1 2s-1 3-4 3z'/>"),
        IconName::Type => ("0 0 24 24", "<path d='M4 7V4h16v3'/><path d='M9 20h6'/><path d='M12 4v16'/>"),
        IconName::Motion => ("0 0 24 24", "<path d='M2 12h3l2.5 6 4-13L14 17l1.5-5H22'/>"),
        IconName::More => ("0 0 24 24", "<circle cx='5' cy='12' r='1.5'/><circle cx='12' cy='12' r='1.5'/><circle cx='19' cy='12' r='1.5'/>"),
        IconName::Keyboard => ("0 0 24 24", "<path d='M2 6a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2z'/><path d='M6 10h.01 M10 10h.01 M14 10h.01 M18 10h.01 M6 14h.01 M18 14h.01 M10 14h4'/>"),
        IconName::Pin => ("0 0 24 24", "<path d='M12 17v5'/><path d='M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z'/>"),
        IconName::Library => ("0 0 24 24", "<path d='M4 19.5A2.5 2.5 0 0 1 6.5 17H20'/><path d='M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z'/>"),
        IconName::Drop => ("0 0 24 24", "<path d='M12 3v11'/><path d='m7 11 5 5 5-5'/><path d='M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2'/>"),
        IconName::ChevronUp => ("0 0 24 24", "<path d='m18 15-6-6-6 6'/>"),
        IconName::ChevronDown => ("0 0 24 24", "<path d='m6 9 6 6 6-6'/>"),
        IconName::Undo => (
            "0 0 24 24",
            "<path d='M9 14 4 9l5-5'/><path d='M4 9h10.5a5.5 5.5 0 0 1 0 11H11'/>",
        ),
        IconName::Reload => (
            "0 0 24 24",
            "<path d='M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8'/><path d='M21 3v5h-5'/>",
        ),
        IconName::Settings => (
            "0 0 24 24",
            "<path d='M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z'/><circle cx='12' cy='12' r='3'/>",
        ),
        IconName::Layout => (
            "0 0 24 24",
            "<rect width='18' height='7' x='3' y='3' rx='1'/><rect width='9' height='7' x='3' y='14' rx='1'/><rect width='5' height='7' x='16' y='14' rx='1'/>",
        ),
        IconName::Minus => ("0 0 24 24", "<path d='M5 12h14'/>"),
        IconName::WindowMinimize => ("0 0 24 24", "<path d='M5 12h14'/>"),
        IconName::WindowMaximize => ("0 0 24 24", "<rect x='5' y='5' width='14' height='14' rx='1'/>"),
        IconName::WindowRestore => ("0 0 24 24", "<rect x='8' y='8' width='11' height='11' rx='1'/><path d='M5 15V6a1 1 0 0 1 1-1h9'/>"),
    }
}

#[component]
pub fn Icon(
    name: IconName,
    #[prop(optional)] size: Option<u16>,
    #[prop(optional, into)] class: Option<String>,
) -> impl IntoView {
    let (view_box, paths) = icon_data(name);
    let px = size.unwrap_or(16);
    view! {
        <svg
            width=px
            height=px
            viewBox=view_box
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            class=class.unwrap_or_default()
            inner_html=paths
        />
    }
}
