//! The reader workspace's knobs — the Workspace tab's schema.

use serde::{Deserialize, Serialize};

/// What a click on a file in the rail's Library panel does. Dragging a file
/// onto the workspace always opens a split, whatever this says; the click is
/// the reader's own choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryClick {
    /// Open the file in the focused pane, in place of what it shows.
    #[default]
    Replace,
    /// Open the file in a new pane beside the focused one.
    Split,
    /// Nothing: files are dragged, never clicked open. The keyboard's Enter
    /// still opens beside, so a keyboard user is never stranded.
    DragOnly,
}

/// The active pane outline colour. Auto follows the reader's current accent;
/// custom uses the separately persisted six-digit RGB value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaneOutlineColor {
    #[default]
    Auto,
    Red,
    Yellow,
    Green,
    Blue,
    Custom,
}

impl PaneOutlineColor {
    /// Resolve a palette choice to CSS. Auto is returned as `None` so the
    /// reader can keep following its live accent token.
    pub fn resolve(self, custom: &str) -> Option<&str> {
        match self {
            Self::Auto => None,
            Self::Red => Some("#e56b64"),
            Self::Yellow => Some("#e8c449"),
            Self::Green => Some("#6fd58c"),
            Self::Blue => Some("#6ba3f5"),
            Self::Custom => Some(custom),
        }
    }
}

/// Shape of a pane's visible box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaneCorners {
    #[default]
    Square,
    Rounded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkspaceSettings {
    pub library_click: LibraryClick,
    /// Independent theme per pane (split workspaces): while a split is on
    /// screen, each pane shows its own look and the shared chrome keeps the
    /// remembered global theme. Persisted as the toggle's rest state, so the
    /// mode stands down with one pane left and comes back with the next
    /// split; the per-pane colours themselves are temporary. The last pane's
    /// colour is promoted to the window theme as the split collapses.
    pub independent_themes: bool,
    /// With independent themes on, keep Light / Dark / Dim shared: each pane
    /// keeps its own colour, but switching the mode switches every pane. Off,
    /// the mode is per pane too, like the rest of its look.
    pub shared_base_mode: bool,
    /// Independent page texture per pane (split workspaces): each pane keeps
    /// its own texture mode and its own opacity / pitch, while the colour
    /// stays the window's. The texture family alone — a page's pattern is a
    /// decision about the paper, not about the look, and reading a Lined PDF
    /// beside a plain-text page wants exactly that. Same lifetime as
    /// `independent_themes`: persisted as the toggle's rest state, in effect
    /// only while a split is on screen, and the last pane's texture is
    /// promoted to the window theme as the split collapses.
    pub independent_textures: bool,
    /// Active-pane focus outline width, in CSS pixels. Zero hides the outline.
    pub pane_outline_width: u8,
    /// Active-pane outline palette selection.
    pub pane_outline_color: PaneOutlineColor,
    /// User-picked outline colour, used only when `pane_outline_color` is
    /// Custom. Kept as a CSS-safe six-digit hex value.
    pub pane_outline_custom: String,
    /// Space between adjacent pane boxes and around all four workspace edges.
    pub pane_gap: u8,
    /// Cast a soft shadow from each pane box, not from the PDF page surface.
    pub pane_shadow: bool,
    /// Pane box corner shape.
    pub pane_corners: PaneCorners,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            library_click: LibraryClick::default(),
            independent_themes: false,
            shared_base_mode: true,
            independent_textures: false,
            pane_outline_width: 2,
            pane_outline_color: PaneOutlineColor::Auto,
            pane_outline_custom: "#6ba3f5".into(),
            pane_gap: 0,
            pane_shadow: false,
            pane_corners: PaneCorners::Square,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_workspace_blobs_load_the_new_split_defaults() {
        let s: WorkspaceSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.library_click, LibraryClick::Replace);
        assert!(!s.independent_themes);
        assert!(s.shared_base_mode);
        assert!(!s.independent_textures);
        assert_eq!(s.pane_outline_width, 2);
        assert_eq!(s.pane_outline_color, PaneOutlineColor::Auto);
        assert_eq!(s.pane_gap, 0);
        assert!(!s.pane_shadow);
        assert_eq!(s.pane_corners, PaneCorners::Square);

        let s: WorkspaceSettings = serde_json::from_str(r#"{"libraryClick":"dragOnly"}"#).unwrap();
        assert_eq!(s.library_click, LibraryClick::DragOnly);
    }

    #[test]
    fn outline_palette_resolves_auto_and_named_colours() {
        assert_eq!(PaneOutlineColor::Auto.resolve("#000000"), None);
        assert_eq!(PaneOutlineColor::Red.resolve("#000000"), Some("#e56b64"));
        assert_eq!(PaneOutlineColor::Custom.resolve("#123abc"), Some("#123abc"));
    }
}

// only the changed file was rewritten
