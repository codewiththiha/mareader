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
        | U::FindStep { .. } => true,
        // Settings, menus and pages open over any page.
        U::ShowSettings
        | U::ShowShortcuts
        | U::ShowAppearance
        | U::ShowViewMenu
        | U::ShowLibrary
        | U::ShowReading
        | U::ShowDictionary
        | U::ShowFind
        | U::ShowOpenDialog(_)
        | U::LibraryDialog(_)
        | U::Window(_)
        | U::CloseSettings => false,
        // Notices show anywhere.
        U::Notify(_) | U::CloseToast(_) | U::ClearToasts => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_engine::pane::ViewMode;

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
