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
    Sepia,
    Green,
    Night,
    Dim,
    Texture,
    Noise,
    Close,
    Check,
    SinglePage,
    Continuous,
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
        IconName::Sepia => ("0 0 24 24", "<circle cx='12' cy='12' r='9'/><path d='M12 3c-2.5 3-2.5 15 0 18'/>"),
        IconName::Green => ("0 0 24 24", "<path d='M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.5 19 2c1 2 2 4.2 2 8 0 5.5-4.8 10-10 10z'/><path d='M2 21c0-3 1.9-5.5 3.5-7'/>"),
        IconName::Night => ("0 0 24 24", "<path d='M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9z'/><path d='M19 3v4M21 5h-4'/>"),
        IconName::Dim => ("0 0 24 24", "<circle cx='12' cy='12' r='9'/><path d='M12 3v18'/>"),
        IconName::Texture => ("0 0 24 24", "<rect x='4' y='4' width='7' height='7' rx='1'/><rect x='13' y='4' width='7' height='7' rx='1'/><rect x='4' y='13' width='7' height='7' rx='1'/><rect x='13' y='13' width='7' height='7' rx='1'/>"),
        IconName::Noise => ("0 0 24 24", "<path d='M2 12c1.5 0 1.5-4 3-4s1.5 8 3 8 1.5-12 3-12 1.5 16 3 16 1.5-8 3-8 1.5 4 3 4'/>"),
        IconName::Close => ("0 0 24 24", "<path d='M18 6 6 18M6 6l12 12'/>"),
        IconName::Check => ("0 0 24 24", "<path d='M20 6 9 17l-5-5'/>"),
        IconName::SinglePage => ("0 0 24 24", "<rect x='4' y='3' width='16' height='18' rx='2'/><path d='M4 9h16'/>"),
        IconName::Continuous => ("0 0 24 24", "<rect x='4' y='3' width='16' height='4' rx='1'/><rect x='4' y='10' width='16' height='4' rx='1'/><rect x='4' y='17' width='16' height='4' rx='1'/>"),
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
