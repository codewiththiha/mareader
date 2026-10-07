//! The reader workspace's knobs — the Workspace tab's schema.

use serde::{Deserialize, Serialize};

/// What a click on a file in the Library panel does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryClick {
    /// Open the file in the focused pane, in place of what it shows.
    #[default]
    Replace,
    /// Open the file in a new pane beside the focused one.
    Split,
    /// Nothing: files are dragged, never clicked open.
    DragOnly,
}

/// The active pane outline colour: auto, or a custom hex.
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
    /// Resolve a palette choice to CSS; Auto answers `None`.
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
    /// Independent theme per pane: each pane shows its own look while split.
    pub independent_themes: bool,
    /// With independent themes on, keep the base mode shared.
    pub shared_base_mode: bool,
    /// Independent page texture per pane; the colour stays the window's.
    pub independent_textures: bool,
    /// Active-pane focus outline width, in CSS pixels. Zero hides the outline.
    pub pane_outline_width: u8,
    /// Active-pane outline palette selection.
    pub pane_outline_color: PaneOutlineColor,
    /// User-picked outline colour, used only for Custom.
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
