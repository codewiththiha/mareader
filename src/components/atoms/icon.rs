//! Inline SVG icon sprite (lucide-style strokes). Renders via inner_html so we
//! never need to touch the svg element nodes.

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
    Continuous,
    // --- phase 2/3 surface (consumed by floating search / appearance menu /
    // MoreMenu); unused until those units land, hence the allow. ---
    #[allow(dead_code)] // consumed in phase 2 (floating_search)
    Menu,
    #[allow(dead_code)] // consumed in phase 3 (appearance_menu)
    Palette,
    #[allow(dead_code)] // consumed in phase 3 (more_menu)
    More,
    #[allow(dead_code)] // consumed in phase 3 (more_menu fullscreen)
    Fullscreen,
    #[allow(dead_code)] // consumed in phase 3 (more_menu print)
    Print,
    #[allow(dead_code)] // consumed in phase 3 (more_menu shortcuts)
    Keyboard,
    // Back-to-library (the recent-books shelf).
    Library,
    // Drag-and-drop feedback overlay.
    Drop,
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
        IconName::Check => ("0 0 24 24", "<path d='M20 6 9 17l-5-5'/>"),
        IconName::SinglePage => ("0 0 24 24", "<rect x='4' y='3' width='16' height='18' rx='2'/><path d='M4 9h16'/>"),
        IconName::Continuous => ("0 0 24 24", "<rect x='4' y='3' width='16' height='4' rx='1'/><rect x='4' y='10' width='16' height='4' rx='1'/><rect x='4' y='17' width='16' height='4' rx='1'/>"),
        IconName::Menu => ("0 0 24 24", "<path d='M4 6h16 M4 12h16 M4 18h16'/>"),
        IconName::Palette => ("0 0 24 24", "<path d='M12 22a10 10 0 1 1 10-10c0 2-1.5 3-3 3h-2a2 2 0 0 0-2 2c0 1 .5 1.5 1 2s-1 3-4 3z'/>"),
        IconName::More => ("0 0 24 24", "<circle cx='5' cy='12' r='1.5'/><circle cx='12' cy='12' r='1.5'/><circle cx='19' cy='12' r='1.5'/>"),
        IconName::Fullscreen => ("0 0 24 24", "<path d='M8 3H5a2 2 0 0 0-2 2v3 M16 3h3a2 2 0 0 1 2 2v3 M8 21H5a2 2 0 0 1-2-2v-3 M16 21h3a2 2 0 0 0 2-2v-3'/>"),
        IconName::Print => ("0 0 24 24", "<path d='M6 9V2h12v7'/><path d='M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2'/><path d='M6 14h12v8H6z'/>"),
        IconName::Keyboard => ("0 0 24 24", "<path d='M2 6a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2z'/><path d='M6 10h.01 M10 10h.01 M14 10h.01 M18 10h.01 M6 14h.01 M18 14h.01 M10 14h4'/>"),
        IconName::Library => ("0 0 24 24", "<path d='M4 19.5A2.5 2.5 0 0 1 6.5 17H20'/><path d='M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z'/>"),
        IconName::Drop => ("0 0 24 24", "<path d='M12 3v11'/><path d='m7 11 5 5 5-5'/><path d='M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2'/>"),
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
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            class=class.unwrap_or_default()
            inner_html=paths
        />
    }
}
