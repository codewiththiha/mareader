//! What the app is asked to do, as data.
//!
//! Every variant here means something with no window on screen: it changes
//! what the next launch shows, what is on disk, or what is heard. Anything
//! that only moves pixels is a UI command, held by the layer above; the two
//! meet in that layer's `Command` union.

use std::path::PathBuf;

use mareader_appearance::{Appearance, Rgb};
use mareader_library::{BookId, Pen, View};
use mareader_settings::{DictionarySettings, Settings};
use mareader_speech::{Model, Spot};
use mareader_workspace::{Bounds, DividerPath, Edge, PaneId, SplitAxis};

use crate::appearance_menu::QuickText;
use crate::dict::{Action as DictAction, Download};
use crate::documents::{OpenRequest, Placement};
use crate::export::Format;
use crate::library::Action as LibraryAction;
use crate::page_ai::Action as PageAiAction;
use crate::pane::{Arrow, ViewMode, ZoomCommand};
use crate::read_aloud::ReadFrom;
use crate::route::Route;
use crate::settings::Tab;
use crate::study::Action as StudyAction;

/// Whether split panes take their own colour, texture, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Independence {
    pub themes: bool,
    pub textures: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Background work for the AI settings: keys, models, a trial.
    #[cfg(not(target_arch = "wasm32"))]
    Ai(mareader_settings::Job),
    /// An arrow key over the active document: a turn, or the scroll's.
    Arrow(Arrow),
    /// The word levels' dataset, at the folder a shot or the settings name.
    #[cfg(not(target_arch = "wasm32"))]
    AttachVocab(PathBuf),
    /// Shift+A: the active pane's auto-scroll on or off.
    AutoScroll,
    /// Executes a sequence of JSON commands in order.
    Batch(Vec<crate::journal::Step>),
    /// Bookmarks the pane's page, or takes its bookmark back; none is
    /// the focused pane.
    Bookmark(Option<PaneId>),
    /// Clears the active text selection in the pane.
    ClearSelection(PaneId),
    ClosePane(PaneId),
    /// Puts the text of the pane's `pages` on the clipboard.
    CopyPages {
        pane: PaneId,
        pages: Vec<usize>,
    },
    /// F6: focus moves to the next or previous pane, in reading order.
    CyclePane {
        forward: bool,
    },
    /// Deletes one of the user's presets.
    DeletePreset(String),
    /// Packs to download or remove, and the lookup card.
    Dict(DictAction),
    /// Explains the word selected in the pane, marking it.
    Explain(PaneId),
    /// Asks where to save an export, then writes it.
    Export {
        name: String,
        format: Format,
        contents: String,
    },
    /// Home and End: the active document's first or last page.
    FirstPage,
    FocusPane(PaneId),
    /// Resets live panes so an erased location cannot be saved back.
    ForgetPositions {
        books: Vec<BookId>,
        paths: Vec<PathBuf>,
    },
    /// Shows `page` of the pane's document, from its start.
    GoTo {
        pane: PaneId,
        page: usize,
    },
    /// Highlights the pane's selection; a colour if one was picked.
    Highlight {
        pane: PaneId,
        pen: Pen,
        color: Option<Rgb>,
    },
    LastPage,
    /// Folders, scopes and finding lost books.
    Library(LibraryAction),
    /// Looks the pane's selection up in the dictionaries.
    LookUp(PaneId),
    /// A lifted pane let go on another's edge: it moves there, in `area`.
    MovePane {
        pane: PaneId,
        other: PaneId,
        edge: Edge,
        area: Bounds,
    },
    /// Highlights the pane's selection and opens a note on it.
    Note(PaneId),
    /// Shows a file in a pane, replacing what it showed.
    Open(OpenRequest),
    /// Opens a library book as Settings › Panes says, or focuses it.
    OpenBook(BookId),
    OpenSettings(Tab),
    /// A page key: nearly a screen along a strip, a turn when paged.
    Page {
        forward: bool,
    },
    /// The page AI's menus: a task to run, a result to show.
    PageAi(PageAiAction),
    /// A library book placed from the rail: clicked, or dropped on a pane.
    PlaceBook {
        book: BookId,
        pane: Option<PaneId>,
        placement: Placement,
    },
    /// Pins where reading aloud will start, to be moved or let rest.
    PlaceReadMarker {
        pane: PaneId,
        spot: Spot,
    },
    /// The pronunciation download, beside the word levels.
    Pronunciation(Download),
    /// Reads the pane's book aloud from a place, else its page.
    ReadAloud {
        pane: PaneId,
        from: ReadFrom,
    },
    /// Drives the read-aloud player controls (pause, skip, seek, stop).
    ReadAloudAct(crate::read_aloud::player::Act),
    /// Settings at a model's voices in `language`, opened and lit.
    RevealVoice(&'static Model, &'static str),
    /// Keeps `look` as a preset, in a section when `group` names one.
    SavePreset {
        name: String,
        group: String,
        look: Appearance,
    },
    /// Says a card's word in the reading voice; stops it.
    SayWord(String),
    /// A word in a voice other than the one picked, for its language.
    SayWordWith {
        word: String,
        voice: String,
    },
    /// Searches the pane's open document for `query`.
    Search {
        pane: PaneId,
        query: String,
    },
    /// Selects the first `text` on `page` in the pane, as a drag does.
    SelectText {
        pane: PaneId,
        page: usize,
        text: String,
    },
    /// Every tick of an appearance control; restyles on the same frame.
    SetAppearance(Appearance),
    /// The Dictionary panel's choices, saved with the settings.
    SetDictionary(Box<DictionarySettings>),
    /// The split's per-pane modes, from the appearance popover.
    SetIndependence(Independence),
    /// The ink dial, 10 to 100 %: how dark a book's text stays.
    SetInkContrast(f32),
    /// The library page's layout and order.
    SetLibraryView(View),
    /// The popover's text dials for the active book.
    SetQuickText(QuickText),
    /// A divider drag; sent every frame it moves.
    SetRatio(DividerPath, f32),
    /// Every edit in the settings window; applies on the same frame.
    SetSettings(Box<Settings>),
    /// One setting, by its dotted path, to its JSON value.
    ///
    /// The channel a dial uses when no variant names it: `text.size_pct` to
    /// `170`. Bad paths and bad values are refused, not applied.
    SetField {
        path: String,
        value: String,
    },
    /// Floating over the panes, or docked beside them.
    SetSidebarOverlay(bool),
    SetTitlebarPinned(bool),
    /// Switches which page fills the window: reader, library or dictionary.
    ShowRoute(Route),
    Split(SplitAxis),
    StopReadAloud,
    StopSaying,
    StopVoiceTry,
    /// The word card's and the mark menu's asks.
    Study(StudyAction),
    /// A lifted pane let go on another: the two trade places.
    SwapPanes {
        pane: PaneId,
        other: PaneId,
    },
    /// Plays or pauses the reading; starts the focused book if none.
    ToggleReadAloud,
    /// Says a line in the voice settings pick; stops that line.
    TryVoice,
    /// Says a line in this voice of the voice tab's engine.
    TryVoiceOf(String),
    /// The previous or next page, or spread, of the pane's document.
    Turn {
        pane: PaneId,
        forward: bool,
    },
    /// The active pane's view mode, kept on the page read.
    View(ViewMode),
    /// The word-level dataset.
    Vocab(Download),
    /// A voice model or voice to download, pause, cancel or remove.
    VoiceModel(mareader_speech::Item, Download),
    /// Copies a word list of the pane's `pages`, with the instant options.
    WordList {
        pane: PaneId,
        pages: Vec<usize>,
    },
    /// Opens the word list window on the pane's `pages`.
    WordListWindow {
        pane: PaneId,
        pages: Vec<usize>,
    },
    /// Zooms the active pane's document.
    Zoom(ZoomCommand),
}
