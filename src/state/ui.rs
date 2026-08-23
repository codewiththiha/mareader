//! App chrome state: which sidebar panel is open. This is UI chrome state,
//! not viewer state — the reader-side rendering code receives it as a plain
//! signal when it needs to know (e.g. which panel is being shown), it never
//! owns it.


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    None,
    Outline,
    Thumbs,
}

