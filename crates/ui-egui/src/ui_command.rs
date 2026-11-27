//! What a view can ask for: the engine's commands, and chrome.
//!
//! `Command` is what means something with no window on screen; `UiCommand`
//! only moves what is on it. A view pushes `Sent`; the app hands an
//! command to the engine and carries a `UiCommand` out itself, in egui.

use std::path::PathBuf;

use egui::{Context, ViewportCommand};
use mareader_engine::command::Command;
use mareader_engine::documents::Placement;
use mareader_engine::route::Route;
use mareader_engine::toast::Toast;
use mareader_library::{BookId, ShelfId};

use crate::sidebar::Panel;

/// File-picker dialogs for library import and relink (Law 6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryDialog {
    Folder,
    SearchFolder(BookId),
    Import,
    From(PathBuf),
    Into(Option<ShelfId>),
    File(BookId),
}

/// What the window buttons do: chrome, and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    Minimize,
    ToggleMaximize,
    Close,
    StartDrag,
    ToggleFullscreen,
}

impl WindowCommand {
    /// What the window is told; the toggles read how it stands now.
    pub fn viewport(self, ctx: &Context) -> ViewportCommand {
        match self {
            Self::Minimize => ViewportCommand::Minimized(true),
            Self::ToggleMaximize => {
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ViewportCommand::Maximized(!maximized)
            }
            Self::Close => ViewportCommand::Close,
            Self::StartDrag => ViewportCommand::StartDrag,
            Self::ToggleFullscreen => {
                let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ViewportCommand::Fullscreen(!full)
            }
        }
    }
}

/// Everything a view can ask for: the engine's commands, and chrome.
#[derive(Clone, Debug, PartialEq)]
pub enum Sent {
    /// Changes something beyond this window: routed to the engine.
    Engine(Command),
    /// Moves something on screen only.
    Ui(UiCommand),
}

impl From<Command> for Sent {
    fn from(command: Command) -> Self {
        Self::Engine(command)
    }
}

impl From<UiCommand> for Sent {
    fn from(ui: UiCommand) -> Self {
        Self::Ui(ui)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiCommand {
    /// Distraction-free reading: every bar hidden until asked back.
    Chromeless,
    /// Closes every toast.
    ClearToasts,
    CloseSettings,
    CloseSidebar,
    /// Closes one toast, by its stack id.
    CloseToast(u64),
    /// Opens a library book and finds `quote` in it: a Space sentence.
    FindInBook {
        book: mareader_library::BookId,
        quote: String,
    },
    /// Opens a vocabulary occurrence and remembers the row to return to.
    FindVocabInBook {
        book: mareader_library::BookId,
        quote: String,
        row: u128,
    },
    /// Opens a note and remembers where to return in Space.
    OpenSpaceNote {
        book: mareader_library::BookId,
        mark: mareader_library::MarkId,
    },
    /// Opens a saved AI selection and remembers its Space row.
    OpenSpaceAi {
        book: mareader_library::BookId,
        id: mareader_library::AiSelectionId,
    },
    /// Opens a Space room on one book's notes or words: a library card's.
    SpaceFor {
        room: mareader_engine::space::Room,
        book: mareader_library::BookId,
    },
    /// F3 and Cmd+G: the next or previous match, opening the bar if shut.
    FindStep {
        forward: bool,
    },
    /// Opens a system file or folder picker for a library action.
    LibraryDialog(LibraryDialog),
    /// Shows a toast on top of the stack.
    Notify(Toast),
    /// Opens the rail on a panel, or on the last one shown.
    OpenSidebar(Option<Panel>),
    /// Sets a page task's continuous default without starting a run.
    PageContinuous(mareader_settings::PageTask, bool),
    /// Opens the Appearance popover (the keymap's route to it).
    ShowAppearance,
    /// The Dictionary page, or back to where it was opened from.
    ShowDictionary,
    /// The find bar over the focused document; else the library.
    ShowFind,
    /// The library page in place of the panes, or back to them.
    ShowLibrary,
    /// The system open dialog, the pick placed as asked.
    ShowOpenDialog(Placement),
    /// Opens the command palette, or closes it when already open.
    ShowPalette,
    /// Back to the pane reading aloud, turned to the words read.
    ShowReading,
    /// The Search buttons: the find bar on, or off if open.
    ShowSearch,
    /// Opens the settings window where it was left.
    ShowSettings,
    /// Opens the keyboard shortcut cheat sheet, or shuts it.
    ShowShortcuts,
    /// Opens the title bar's View & tools menu.
    ShowViewMenu,
    ToggleSidebar,
    Window(WindowCommand),
    /// Switches the open word card between Dictionary and AI in context.
    WordCardMode(crate::word_card::Mode),
    /// Swaps the open Dictionary card's source and target languages.
    WordCardSwapLang,
    /// Looks up the open Dictionary card's word in another target language.
    WordCardTryLang(String),
    /// Opens the editor on an AI dictionary entry.
    EditAi(Box<mareader_dict::Saved>),
}

/// The frame's command queue, handed to every view.
pub type Commands = Vec<Sent>;

/// Which commands each page takes: chrome's view of `Route`.
pub trait Allows {
    /// Whether `command` means anything here; the rest are dropped.
    fn allows(self, command: &Sent) -> bool;
}

impl Allows for Route {
    fn allows(self, command: &Sent) -> bool {
        match command {
            Sent::Engine(intent) => self.takes(intent),
            Sent::Ui(ui) => self == Route::Reader || !chrome_reads(ui),
        }
    }
}

fn chrome_reads(ui: &UiCommand) -> bool {
    use UiCommand as U;
    // Exhaustive, so each new UI command says where it belongs.
    match ui {
        // The rail and the bar's folding tools act on panes.
        U::ToggleSidebar
        | U::OpenSidebar(_)
        | U::CloseSidebar
        | U::ShowSearch
        | U::Chromeless
        | U::FindStep { .. }
        | U::WordCardMode(_)
        | U::WordCardSwapLang
        | U::WordCardTryLang(_)
        | U::PageContinuous(_, _) => true,
        // Settings, menus and pages open over any page.
        U::ShowSettings
        | U::ShowShortcuts
        | U::ShowPalette
        | U::ShowAppearance
        | U::ShowViewMenu
        | U::ShowLibrary
        | U::ShowReading
        | U::ShowDictionary
        | U::ShowFind
        | U::FindInBook { .. }
        | U::FindVocabInBook { .. }
        | U::OpenSpaceNote { .. }
        | U::OpenSpaceAi { .. }
        | U::SpaceFor { .. }
        | U::ShowOpenDialog(_)
        | U::LibraryDialog(_)
        | U::Window(_)
        | U::EditAi(_)
        | U::CloseSettings => false,
        // Notices show anywhere.
        U::Notify(_) | U::CloseToast(_) | U::ClearToasts => false,
    }
}

/// Static metadata for a UI-only command in the palette and control channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiCommandMeta {
    pub id: &'static str,
    pub title: &'static str,
    pub label: &'static str,
    pub menu: Option<&'static str>,
    pub shortcut: Option<&'static str>,
}

/// Canonical catalog of UI commands, mirrored by `cargo xtask ui-commands`.
pub const UI_COMMANDS: &[UiCommandMeta] = &[
    UiCommandMeta {
        id: "ui.palette",
        title: "Command Palette",
        label: "Open or close the command palette.",
        menu: Some("App"),
        shortcut: Some("Mod+K"),
    },
    UiCommandMeta {
        id: "ui.open-dialog",
        title: "Open File...",
        label: "Open the system file picker.",
        menu: Some("File"),
        shortcut: Some("Mod+O"),
    },
    UiCommandMeta {
        id: "ui.sidebar-toggle",
        title: "Toggle Sidebar",
        label: "Show or hide the reading sidebar.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.sidebar-close",
        title: "Close Sidebar",
        label: "Hide the reading sidebar.",
        menu: None,
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.sidebar-thumbnails",
        title: "Thumbnails Panel",
        label: "Open the thumbnails sidebar panel.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.sidebar-outline",
        title: "Outline Panel",
        label: "Open the outline sidebar panel.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.sidebar-notes",
        title: "Notes Panel",
        label: "Open the notes sidebar panel.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.sidebar-library",
        title: "Library Sidebar Panel",
        label: "Open the library sidebar panel.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.chromeless",
        title: "Distraction-Free Mode",
        label: "Hide or restore all reader chrome.",
        menu: Some("View"),
        shortcut: Some("Mod+Shift+F"),
    },
    UiCommandMeta {
        id: "ui.fullscreen",
        title: "Toggle Full Screen",
        label: "Enter or leave full-screen mode.",
        menu: Some("View"),
        shortcut: Some("F11"),
    },
    UiCommandMeta {
        id: "ui.find",
        title: "Find",
        label: "Open the find bar for the active view.",
        menu: Some("Find"),
        shortcut: Some("Mod+F"),
    },
    UiCommandMeta {
        id: "ui.find-next",
        title: "Next Match",
        label: "Jump to the next search match.",
        menu: Some("Find"),
        shortcut: Some("Mod+G"),
    },
    UiCommandMeta {
        id: "ui.find-prev",
        title: "Previous Match",
        label: "Jump to the previous search match.",
        menu: Some("Find"),
        shortcut: Some("Mod+Shift+G"),
    },
    UiCommandMeta {
        id: "ui.search",
        title: "Toggle Search Bar",
        label: "Show or hide the document search bar.",
        menu: Some("Find"),
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.library",
        title: "Toggle Library",
        label: "Switch between the library and the reader.",
        menu: Some("App"),
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.dictionary",
        title: "Toggle Dictionary",
        label: "Switch between the dictionary and the reader.",
        menu: Some("App"),
        shortcut: Some("Mod+Shift+D"),
    },
    UiCommandMeta {
        id: "ui.reading",
        title: "Show Reading Pane",
        label: "Jump back to the pane reading aloud.",
        menu: Some("Reading"),
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.appearance",
        title: "Appearance Popover",
        label: "Open the appearance popover.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.view-menu",
        title: "View & Tools Menu",
        label: "Open the View & tools menu.",
        menu: Some("View"),
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.settings",
        title: "Settings",
        label: "Open the settings window.",
        menu: Some("App"),
        shortcut: Some("Mod+,"),
    },
    UiCommandMeta {
        id: "ui.settings-close",
        title: "Close Settings",
        label: "Close the settings window.",
        menu: None,
        shortcut: None,
    },
    UiCommandMeta {
        id: "ui.shortcuts",
        title: "Keyboard Shortcuts",
        label: "Show or hide the keyboard shortcuts sheet.",
        menu: Some("App"),
        shortcut: Some("Mod+/"),
    },
    UiCommandMeta {
        id: "ui.clear-toasts",
        title: "Clear Notifications",
        label: "Dismiss all visible toast notifications.",
        menu: None,
        shortcut: None,
    },
];

/// Looks up UI command metadata by stable id.
pub fn ui_spec(id: &str) -> Option<&'static UiCommandMeta> {
    UI_COMMANDS.iter().find(|meta| meta.id == id)
}

/// Parses a `ui.*` command id into a concrete [`UiCommand`].
pub fn parse_ui_command(id: &str) -> Result<UiCommand, String> {
    match id {
        "ui.palette" => Ok(UiCommand::ShowPalette),
        "ui.open-dialog" => Ok(UiCommand::ShowOpenDialog(Placement::Into)),
        "ui.sidebar-toggle" => Ok(UiCommand::ToggleSidebar),
        "ui.sidebar-close" => Ok(UiCommand::CloseSidebar),
        "ui.sidebar-thumbnails" => Ok(UiCommand::OpenSidebar(Some(Panel::Thumbnails))),
        "ui.sidebar-outline" => Ok(UiCommand::OpenSidebar(Some(Panel::Outline))),
        "ui.sidebar-notes" => Ok(UiCommand::OpenSidebar(Some(Panel::Notes))),
        "ui.sidebar-library" => Ok(UiCommand::OpenSidebar(Some(Panel::Library))),
        "ui.chromeless" => Ok(UiCommand::Chromeless),
        "ui.fullscreen" => Ok(UiCommand::Window(WindowCommand::ToggleFullscreen)),
        "ui.find" => Ok(UiCommand::ShowFind),
        "ui.find-next" => Ok(UiCommand::FindStep { forward: true }),
        "ui.find-prev" => Ok(UiCommand::FindStep { forward: false }),
        "ui.search" => Ok(UiCommand::ShowSearch),
        "ui.library" => Ok(UiCommand::ShowLibrary),
        "ui.dictionary" => Ok(UiCommand::ShowDictionary),
        "ui.reading" => Ok(UiCommand::ShowReading),
        "ui.appearance" => Ok(UiCommand::ShowAppearance),
        "ui.view-menu" => Ok(UiCommand::ShowViewMenu),
        "ui.settings" => Ok(UiCommand::ShowSettings),
        "ui.settings-close" => Ok(UiCommand::CloseSettings),
        "ui.shortcuts" => Ok(UiCommand::ShowShortcuts),
        "ui.clear-toasts" => Ok(UiCommand::ClearToasts),
        _ => Err(format!("unknown UI command {id:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_engine::pane::ViewMode;
    use std::collections::HashSet;

    #[test]
    fn ui_commands_catalog_is_unique_and_parseable() {
        let mut seen = HashSet::new();
        for meta in UI_COMMANDS {
            assert!(meta.id.starts_with("ui."), "{}", meta.id);
            assert!(!meta.title.is_empty(), "{}", meta.id);
            assert!(!meta.label.is_empty(), "{}", meta.id);
            assert!(seen.insert(meta.id), "duplicate {}", meta.id);
            assert!(parse_ui_command(meta.id).is_ok(), "{}", meta.id);
        }
        assert!(parse_ui_command("ui.nonexistent").is_err());
    }

    #[test]
    fn the_library_drops_what_acts_on_panes() {
        let lib = Route::Library;
        assert!(!lib.allows(&Sent::Ui(UiCommand::ToggleSidebar)));
        assert!(lib.allows(&Sent::Ui(UiCommand::ShowSettings)));
        assert!(!lib.allows(&Sent::Engine(Command::View(ViewMode::Single))));
        assert!(!lib.allows(&Sent::Engine(Command::FirstPage)));
        assert!(lib.allows(&Sent::Ui(UiCommand::ShowLibrary)));
        assert!(lib.allows(&Sent::Ui(UiCommand::ShowViewMenu)));
        assert!(lib.allows(&Sent::Ui(UiCommand::Window(WindowCommand::Close))));
        assert!(Route::Reader.allows(&Sent::Ui(UiCommand::ToggleSidebar)));
    }

    #[test]
    fn the_dictionary_keeps_its_settings_and_drops_reading() {
        let page = Route::Dictionary;
        assert!(page.allows(&Sent::Ui(UiCommand::ShowSettings)));
        assert!(page.allows(&Sent::Ui(UiCommand::ShowDictionary)));
        assert!(page.allows(&Sent::Ui(UiCommand::ShowLibrary)));
        assert!(!page.allows(&Sent::Ui(UiCommand::ToggleSidebar)));
        assert!(!page.allows(&Sent::Engine(Command::Bookmark(None))));
        assert!(!page.has_rail());
    }
}
