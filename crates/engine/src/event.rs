//! What the session reports back; a front end drains it once a frame.

use std::ops::Range;

use mareader_library::MarkId;
use mareader_session::{Anchor, DocId};
use mareader_settings::WordList;
use mareader_speech::{Model, Spot};
use mareader_workspace::PaneId;

use crate::dict::{Action as DictAction, Window as DictWindow};
use crate::export::Format;
use crate::library::Action as LibraryAction;
use crate::page_ai::Action as PageAiAction;
use crate::pane::{Arrow, ViewMode, ZoomCommand};
use crate::route::Route;
use crate::settings::Tab;
use crate::study::Action as StudyAction;
use crate::study::state::Key as StudyKey;
use crate::study::words::Asked;
use crate::toast::Toast;

/// A view-level movement or layout change applied to a pane's viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaneAction {
    Zoom(ZoomCommand),
    SetMode(ViewMode),
    Arrow(Arrow),
    PageKey { forward: bool },
    ToggleDrift,
    GoTo(usize),
}

/// One result of a command that the session cannot show itself.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A note for the reader; the front end decides how it looks.
    Toast(Toast),
    /// The active page changed; overlays of the previous page should settle.
    RouteChanged(Route),
    /// Open the settings window on this tab.
    OpenSettings(Tab),
    /// Reveal a model's voice fold for `language` inside settings.
    RevealVoice {
        model: &'static Model,
        language: &'static str,
    },
    /// Save an exported document where the user chooses.
    Export {
        name: String,
        format: Format,
        contents: String,
    },
    /// Open the word-list window for `pane`'s `pages`.
    WordListWindow {
        pane: PaneId,
        pages: Vec<usize>,
        list: WordList,
    },
    /// F6 stepped past the last pane: the keys go to the parked tab.
    FocusEdgeTab,
    /// F6 moved to a pane: whatever widget held the keys lets go.
    ReleaseFocus,
    /// A pane opened synchronously onto an already-loaded document.
    PaneOpened {
        pane: PaneId,
        page: usize,
        count: usize,
        anchor: Option<Anchor>,
    },
    /// A pane dropped its document or left the tree.
    PaneClosed(PaneId),
    /// An unreferenced document was released; drop its cached page textures.
    ForgetPages(DocId),
    /// Update a pane's front-end viewport to match a command.
    PaneView {
        pane: PaneId,
        action: PaneAction,
        now: f64,
    },
    /// Place a temporary read-aloud pin on `pane`.
    PlaceReadMarker { pane: PaneId, spot: Spot, now: f64 },
    /// Dismiss any active read-aloud pins.
    DismissReadMarkers(f64),
    /// Read-aloud playback started (`fresh` resets front-end bar state).
    ReadAloudStarted { fresh: bool },
    /// Front-end study UI action (cards, pickers, note drafts).
    Study(StudyAction),
    /// AI Dictionary mark and lookup started; open its card beside the word.
    ExplainOpened {
        pane: PaneId,
        key: StudyKey,
        asked: Asked,
    },
    /// Dictionary lookup started; open its card beside `spans`.
    LookUpOpened {
        pane: PaneId,
        word: String,
        ticket: mareader_dict::Ticket,
        spans: Vec<(usize, Range<usize>)>,
        asked: Option<Asked>,
    },
    /// Open floating dictionary window answers `word` instead of a card.
    DictWindowFeed(String),
    /// Selection highlighted for a note; open its draft beside `spans`.
    NoteOpened {
        pane: PaneId,
        id: MarkId,
        made: bool,
        spans: Vec<(usize, Range<usize>)>,
    },
    /// Move or toggle the floating dictionary window.
    DictWindow(DictWindow),
    /// Open the dictionary window focused on `word`.
    Pursue(String),
    /// Front-end dictionary UI action.
    Dict(DictAction),
    /// Front-end page-AI menu or model-backed run action.
    PageAi(PageAiAction),
    /// A skill was asked: its card opens.
    SkillOpened(crate::skills::AskId),
    /// A skill's answer landed; out of sight, a toast offers it.
    SkillLanded(crate::skills::AskId),
    /// Go back to a skill's card, and show it.
    SkillShow(crate::skills::AskId),
    /// Background AI settings job run by the front-end AI runtime.
    #[cfg(not(target_arch = "wasm32"))]
    Ai(mareader_settings::Job),
    /// Front-end library dialog, search focus, or AI organizer action.
    Library(LibraryAction),
    /// The CEFR tagger answered a word's grammatical role.
    Role {
        word: String,
        role: mareader_cefr::Role,
    },
    /// The open word card's dictionary rows named its class.
    Class {
        word: String,
        pos: mareader_study::Pos,
    },
    /// Dictionary packs changed or lookup replies arrived.
    DictReplies {
        changed: bool,
        /// An AI dictionary entry was edited or deleted.
        edited: bool,
        replies: Vec<mareader_dict::Reply>,
    },
    /// An instant word list finished building.
    WordList {
        made: crate::word_list::Made,
        list: WordList,
    },
    /// A verified update was applied and relaunched; close cleanly.
    UpdateRestart,
    /// User asked to view an update; close Settings so modals never stack.
    CloseSettings,
}
