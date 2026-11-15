//! Generated mirror of `crates/ui-egui/src/ui_command.rs::UI_COMMANDS`.
//!
//! Do not edit by hand; regenerate with `cargo xtask ui-commands`.

/// Static metadata for a UI-only command, mirrored without linking `egui`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiCommandInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub label: &'static str,
    pub menu: Option<&'static str>,
    pub shortcut: Option<&'static str>,
}

/// UI-only commands exposed by `mareader-ui-egui`.
pub const UI_COMMANDS: &[UiCommandInfo] = &[
    UiCommandInfo {
        id: "ui.palette",
        title: "Command Palette",
        label: "Open or close the command palette.",
        menu: Some("App"),
        shortcut: Some("Mod+K"),
    },
    UiCommandInfo {
        id: "ui.open-dialog",
        title: "Open File...",
        label: "Open the system file picker.",
        menu: Some("File"),
        shortcut: Some("Mod+O"),
    },
    UiCommandInfo {
        id: "ui.sidebar-toggle",
        title: "Toggle Sidebar",
        label: "Show or hide the reading sidebar.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.sidebar-close",
        title: "Close Sidebar",
        label: "Hide the reading sidebar.",
        menu: None,
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.sidebar-thumbnails",
        title: "Thumbnails Panel",
        label: "Open the thumbnails sidebar panel.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.sidebar-outline",
        title: "Outline Panel",
        label: "Open the outline sidebar panel.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.sidebar-notes",
        title: "Notes Panel",
        label: "Open the notes sidebar panel.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.sidebar-library",
        title: "Library Sidebar Panel",
        label: "Open the library sidebar panel.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.chromeless",
        title: "Distraction-Free Mode",
        label: "Hide or restore all reader chrome.",
        menu: Some("View"),
        shortcut: Some("Mod+Shift+F"),
    },
    UiCommandInfo {
        id: "ui.fullscreen",
        title: "Toggle Full Screen",
        label: "Enter or leave full-screen mode.",
        menu: Some("View"),
        shortcut: Some("F11"),
    },
    UiCommandInfo {
        id: "ui.find",
        title: "Find",
        label: "Open the find bar for the active view.",
        menu: Some("Find"),
        shortcut: Some("Mod+F"),
    },
    UiCommandInfo {
        id: "ui.find-next",
        title: "Next Match",
        label: "Jump to the next search match.",
        menu: Some("Find"),
        shortcut: Some("Mod+G"),
    },
    UiCommandInfo {
        id: "ui.find-prev",
        title: "Previous Match",
        label: "Jump to the previous search match.",
        menu: Some("Find"),
        shortcut: Some("Mod+Shift+G"),
    },
    UiCommandInfo {
        id: "ui.search",
        title: "Toggle Search Bar",
        label: "Show or hide the document search bar.",
        menu: Some("Find"),
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.library",
        title: "Toggle Library",
        label: "Switch between the library and the reader.",
        menu: Some("App"),
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.dictionary",
        title: "Toggle Dictionary",
        label: "Switch between the dictionary and the reader.",
        menu: Some("App"),
        shortcut: Some("Mod+Shift+D"),
    },
    UiCommandInfo {
        id: "ui.reading",
        title: "Show Reading Pane",
        label: "Jump back to the pane reading aloud.",
        menu: Some("Reading"),
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.appearance",
        title: "Appearance Popover",
        label: "Open the appearance popover.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.view-menu",
        title: "View & Tools Menu",
        label: "Open the View & tools menu.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.settings",
        title: "Settings",
        label: "Open the settings window.",
        menu: Some("App"),
        shortcut: Some("Mod+,"),
    },
    UiCommandInfo {
        id: "ui.settings-close",
        title: "Close Settings",
        label: "Close the settings window.",
        menu: None,
        shortcut: None,
    },
    UiCommandInfo {
        id: "ui.shortcuts",
        title: "Keyboard Shortcuts",
        label: "Show or hide the keyboard shortcuts sheet.",
        menu: Some("App"),
        shortcut: Some("Mod+/"),
    },
    UiCommandInfo {
        id: "ui.clear-toasts",
        title: "Clear Notifications",
        label: "Dismiss all visible toast notifications.",
        menu: None,
        shortcut: None,
    },
];
