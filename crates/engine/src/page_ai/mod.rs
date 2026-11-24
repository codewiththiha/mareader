//! What a page asks the AI, and what comes back.

pub mod cards;
pub mod extract;
pub mod follow;
pub mod groups;
pub mod layout;
pub mod ledger;
pub mod ops;
pub mod read;
pub mod state;
pub mod view;

pub use layout::{base_page, pages_of};

use mareader_library::AiPageId;
use mareader_settings::PageTask;
use mareader_workspace::PaneId;

/// One result: kept with the book, or still being written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Saved(AiPageId),
    Run(u64),
}

/// How a task runs: on this page, or on as the reader reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct How {
    pub continuous: bool,
    /// The choice becomes the task's setting.
    pub remember: bool,
}

/// What the page and bubble menus ask of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Run {
        pane: PaneId,
        page: usize,
        task: PageTask,
        how: How,
    },
    /// Translates into language `tag`, remembered for next time.
    Translate {
        pane: PaneId,
        page: usize,
        tag: String,
        how: How,
    },
    /// Ends the pane's continuous run.
    StopFollow(PaneId),
    /// Asks pages picked in the thumbnails, in groups.
    RunPages {
        pane: PaneId,
        pages: Vec<usize>,
        task: PageTask,
        tag: Option<String>,
    },
    /// The page menu for picked pages, at its languages.
    PagesMenu {
        pane: PaneId,
        pages: Vec<usize>,
        /// Where the pointer asked, as x and y.
        at: [f32; 2],
    },
    /// Drops the pane's waiting groups and stops those being written.
    StopPages(PaneId),
    /// Stops one page's run, by its id, leaving the page as it was.
    StopRun(u64),
    /// Shows a result over the page; none shows the page itself.
    Show {
        pane: PaneId,
        page: usize,
        slot: Option<Slot>,
    },
    Redo {
        pane: PaneId,
        page: usize,
        slot: Slot,
    },
    Copy(Slot),
    Delete(Slot),
}
