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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkspaceSettings {
    pub library_click: LibraryClick,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_blob_loads_the_default_click() {
        let s: WorkspaceSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.library_click, LibraryClick::Replace);
        let s: WorkspaceSettings = serde_json::from_str(r#"{"libraryClick":"dragOnly"}"#).unwrap();
        assert_eq!(s.library_click, LibraryClick::DragOnly);
    }
}
