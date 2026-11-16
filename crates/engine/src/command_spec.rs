//! Every command, named and described once, so a window and a terminal can
//! list the same set.
//!
//! The table is the registry `mareader-cli commands` prints and the parity
//! ledger cites by id. A row is a script's contract too: the keys its params
//! hold, the pages that take the command, and the form that builds it from
//! those params (`command_spec/rows.rs` holds the table, and each family's
//! forms are beside the keys they read).

mod acts;
mod json;
mod keeps;
mod marks;
mod meta;
mod names;
mod pages;
mod panes;
mod params;
mod rows;

use serde_json::Value;

use crate::command::Command;
use crate::route::Route;
use crate::session::Session;

/// One command: its stable id, what it says, and what a script sends it.
#[derive(Clone, Copy, Debug)]
pub struct CommandSpec {
    /// Dotted and stable: `pane.split`, the id a ledger or a script names.
    pub id: &'static str,
    /// Short human-facing title for menus and the command palette.
    pub title: &'static str,
    /// One line, in the imperative, saying what happens.
    pub label: &'static str,
    /// Optional menu category for grouping commands in UI surfaces.
    pub menu: Option<&'static str>,
    /// Optional default keyboard shortcut label (e.g. `Mod+D`, `Home`).
    pub shortcut: Option<&'static str>,
    /// The keys a script's params hold, each beside the kind of value it takes.
    ///
    /// A kind ending in `?` may be left out, `a|b` is one of those names, and
    /// `x,y,w,h` is an object of those four numbers. The key `pane` means the
    /// pane in focus when a script leaves it out.
    pub params: &'static [(&'static str, &'static str)],
    /// Whether a page takes the command: `Route::takes` asked of the row,
    /// instead of of a command built to ask it.
    pub enabled: fn(Route) -> bool,
    /// Session-aware enablement check returning `Ok(())` or why it cannot run.
    pub can_run: fn(&Session) -> Result<(), &'static str>,
    /// Whether successful executions are recorded in the session journal.
    pub journal: bool,
    /// The command a script's params build, or the line saying why they do not.
    pub parse: fn(&Session, &Value) -> Result<Command, String>,
}

/// Every command there is, one row each.
pub const SPECS: &[CommandSpec] = rows::ROWS;

/// The row an id names, which is how a script's `exec` finds its command.
pub fn by_id(id: &str) -> Option<&'static CommandSpec> {
    SPECS.iter().find(|spec| spec.id == id)
}

pub(crate) const fn row(
    id: &'static str,
    label: &'static str,
    params: &'static [(&'static str, &'static str)],
    enabled: fn(Route) -> bool,
    parse: fn(&Session, &Value) -> Result<Command, String>,
) -> CommandSpec {
    let info = meta::for_id(id);
    CommandSpec {
        id,
        title: info.title,
        label,
        menu: info.menu,
        shortcut: info.shortcut,
        params,
        enabled,
        can_run: info.can_run,
        journal: info.journal,
        parse,
    }
}

pub(crate) fn anywhere(_: Route) -> bool {
    true
}

pub(crate) fn reader_only(route: Route) -> bool {
    route == Route::Reader
}

impl Command {
    /// This command's stable id.
    pub fn id(&self) -> &'static str {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Command::Ai(..) => "ai.job",
            Command::Arrow(..) => "page.arrow",
            #[cfg(not(target_arch = "wasm32"))]
            Command::AttachVocab(..) => "vocab.attach",
            Command::AutoScroll => "page.autoscroll",
            Command::Batch(..) => "command.batch",
            Command::Bookmark(..) => "mark.bookmark",
            Command::ClearSelection(..) => "mark.clear",
            Command::ClosePane(..) => "pane.close",
            Command::CopyPages { .. } => "page.copy",
            Command::CyclePane { .. } => "pane.cycle",
            Command::DeletePreset(..) => "settings.preset-delete",
            Command::Dict(..) => "dict.act",
            Command::Explain(..) => "word.explain",
            Command::Export { .. } => "export.write",
            Command::FirstPage => "page.first",
            Command::FocusPane(..) => "pane.focus",
            Command::ForgetPositions { .. } => "library.forget",
            Command::GoTo { .. } => "page.go",
            Command::Highlight { .. } => "mark.highlight",
            Command::LastPage => "page.last",
            Command::Library(..) => "library.act",
            Command::LookUp(..) => "word.lookup",
            Command::MovePane { .. } => "pane.move",
            Command::Note(..) => "mark.note",
            Command::Open(..) => "reader.open",
            Command::OpenBook(..) => "reader.open-book",
            Command::OpenSettings(..) => "settings.open",
            Command::Page { .. } => "page.step",
            Command::PageAi(..) => "ai.page",
            Command::PlaceBook { .. } => "reader.place-book",
            Command::PlaceReadMarker { .. } => "read.marker",
            Command::Pronunciation(..) => "dict.pronunciation",
            Command::ReadAloud { .. } => "read.start",
            Command::ReadAloudAct(..) => "read.act",
            Command::RevealVoice(..) => "voice.reveal",
            Command::SavePreset { .. } => "settings.preset-save",
            Command::SayWord(..) => "word.say",
            Command::SayWordWith { .. } => "word.say-with",
            Command::Search { .. } => "page.search",
            Command::SelectText { .. } => "mark.select",
            Command::SetAppearance(..) => "settings.appearance",
            Command::SetDictionary(..) => "dict.settings",
            Command::SetIndependence(..) => "settings.independence",
            Command::SetInkContrast(..) => "settings.ink",
            Command::SetLibraryView(..) => "library.view",
            Command::SetQuickText(..) => "settings.quick-text",
            Command::SetRatio(..) => "pane.ratio",
            Command::SetSettings(..) => "settings.write",
            Command::SetField { .. } => "settings.set",
            Command::SetSidebarOverlay(..) => "settings.sidebar-overlay",
            Command::SetTitlebarPinned(..) => "settings.titlebar-pin",
            Command::ShowRoute(..) => "route.show",
            Command::Split(..) => "pane.split",
            Command::StopReadAloud => "read.stop",
            Command::StopSaying => "word.stop",
            Command::StopVoiceTry => "voice.stop-try",
            Command::Study(..) => "study.act",
            Command::SwapPanes { .. } => "pane.swap",
            Command::ToggleReadAloud => "read.toggle",
            Command::TryVoice => "voice.try",
            Command::TryVoiceOf(..) => "voice.try-of",
            Command::Turn { .. } => "page.turn",
            Command::Update(..) => "update.act",
            Command::View(..) => "pane.view",
            Command::Vocab(..) => "vocab.download",
            Command::VoiceModel(..) => "voice.model",
            Command::WordList { .. } => "word.list",
            Command::WordListWindow { .. } => "word.list-window",
            Command::Zoom(..) => "pane.zoom",
        }
    }

    /// This command's row in [`SPECS`], when it has one.
    ///
    /// A `None` is a missing row, which `tests/specs.rs` fails on; the
    /// lookup does not pretend otherwise.
    pub fn spec(&self) -> Option<&'static CommandSpec> {
        by_id(self.id())
    }
}
