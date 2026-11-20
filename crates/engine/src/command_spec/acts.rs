//! The forms a script sends for a family's asks: an id, a name, its params.
//!
//! Five commands carry an enum of their own — the dictionary's, the study
//! tools', the page AI's, the library's and the AI settings' — so a script
//! spells the ask out in an `"action"` and sends that ask's keys beside it.
//! The names listed here are the ones a script can send; an ask whose params
//! are not a script's to make says so, and names the ones that are.

use serde_json::Value;

use crate::command::Command;
use crate::command_spec::json;
use crate::dict::Action as DictAction;
use crate::library::Action as LibraryAction;
use crate::page_ai::Action as PageAiAction;
use crate::read_aloud::player::Act as ReadAct;
use crate::session::Session;
use crate::study::Action as StudyAction;
use crate::updates::{Action as UpdateAction, MockMode, MockUpdate, UpdatePlatform};

/// The dictionary's asks a script can send.
pub(crate) const DICT: &str = "close_card|pursue";

/// `dict.act`: the lookup card, and a word to go after.
pub(crate) fn dict(_at: &Session, params: &Value) -> Result<Command, String> {
    let name = json::text(params, "action")?;
    let act = match name.as_str() {
        "close_card" => DictAction::CloseCard,
        "pursue" => DictAction::Pursue(json::text(params, "word")?),
        other => return Err(refused("dict.act", other, DICT)),
    };
    Ok(Command::Dict(act))
}

/// The study tools' asks a script can send.
pub(crate) const STUDY: &str = concat!(
    "retry|ask_again|close|pick_page|",
    "pick_all|done_picking|edit_note|",
    "cancel_note|set_note",
);

/// `study.act`: the word card's and the mark menu's asks.
pub(crate) fn study(at: &Session, params: &Value) -> Result<Command, String> {
    let name = json::text(params, "action")?;
    let act = match name.as_str() {
        "retry" => StudyAction::Retry,
        "ask_again" => StudyAction::AskAgain,
        "close" => StudyAction::Close,
        "pick_page" => StudyAction::PickPage,
        "pick_all" => StudyAction::PickAll,
        "done_picking" => StudyAction::DonePicking,
        "edit_note" => StudyAction::EditNote,
        "cancel_note" => StudyAction::CancelNote,
        "set_note" => {
            let text = json::text(params, "text")?;
            let mark = match json::maybe_mark(params, "mark")? {
                Some(id) => id,
                None => {
                    let pane = json::pane(at, params)?;
                    at.latest_mark(pane)
                        .ok_or_else(|| "no highlight mark on that book".to_owned())?
                }
            };
            StudyAction::SetNote(mark, text)
        }
        other => return Err(refused("study.act", other, STUDY)),
    };
    Ok(Command::Study(act))
}

/// The page AI's asks a script can send.
pub(crate) const PAGE_AI: &str = "stop_follow|stop_pages|stop_pill";

/// `ai.page`: the page AI's menus, of which a script can stop one.
pub(crate) fn page_ai(at: &Session, params: &Value) -> Result<Command, String> {
    let name = json::text(params, "action")?;
    let pane = json::pane(at, params)?;
    let act = match name.as_str() {
        "stop_follow" => PageAiAction::StopFollow(pane),
        "stop_pages" => PageAiAction::StopPages(pane),
        "stop_pill" => PageAiAction::StopPill(pane),
        other => return Err(refused("ai.page", other, PAGE_AI)),
    };
    Ok(Command::PageAi(act))
}

/// The read-aloud player's controls a script can send.
pub(crate) const READ_ACT: &str = "back|toggle|next|escape|answer|seek|stop|card|reveal";

/// `read.act`: drives the read-aloud player controls.
pub(crate) fn read_act(_at: &Session, params: &Value) -> Result<Command, String> {
    let name = json::text(params, "action")?;
    let act = match name.as_str() {
        "back" => ReadAct::Back,
        "toggle" => ReadAct::Toggle,
        "next" => ReadAct::Next,
        "escape" => ReadAct::Escape,
        "answer" => ReadAct::Answer(json::flag(params, "read")?),
        "seek" => ReadAct::Seek(json::real(params, "share")?),
        "stop" => ReadAct::Stop,
        "card" => ReadAct::Card(json::flag(params, "card")?),
        "reveal" => ReadAct::Reveal,
        other => return Err(refused("read.act", other, READ_ACT)),
    };
    Ok(Command::ReadAloudAct(act))
}

/// The library's asks a script can send.
pub(crate) const LIBRARY: &str = concat!(
    "show_add|rescan|cancel_asks|",
    "close_sheet|ask_rename_last|",
    "find_again_lost|stop_organizing|",
    "resume_organizing|discard_paused|",
    "restore_paused|apply_organized|",
    "discard_organized|seed_shelves|",
    "runs_demo|import_folder",
);

/// `library.act`: folders, scopes, the sheets and the fixtures.
pub(crate) fn library(_at: &Session, params: &Value) -> Result<Command, String> {
    let name = json::text(params, "action")?;
    let act = match name.as_str() {
        "show_add" => LibraryAction::ShowAdd,
        "rescan" => LibraryAction::Rescan,
        "cancel_asks" => LibraryAction::CancelAsks,
        "close_sheet" => LibraryAction::CloseSheet,
        "ask_rename_last" => LibraryAction::AskRenameLast,
        "find_again_lost" => LibraryAction::FindAgainLost,
        "stop_organizing" => LibraryAction::StopOrganizing,
        "resume_organizing" => LibraryAction::ResumeOrganizing,
        "discard_paused" => LibraryAction::DiscardPaused,
        "restore_paused" => LibraryAction::RestorePaused,
        "apply_organized" => LibraryAction::ApplyOrganized,
        "discard_organized" => LibraryAction::DiscardOrganized,
        "seed_shelves" => LibraryAction::SeedShelves,
        "runs_demo" => LibraryAction::RunsDemo,
        "import_folder" => import_folder(params)?,
        other => return Err(refused("library.act", other, LIBRARY)),
    };
    Ok(Command::Library(act))
}

fn import_folder(params: &Value) -> Result<LibraryAction, String> {
    let root = json::path(params, "root")?;
    let mut opts = mareader_library::Options::default();
    if let Some(min_size) = json::maybe_wide(params, "min_size")? {
        opts.min_size = min_size;
    }
    if let Some(in_place) = json::maybe_flag(params, "in_place")? {
        opts.in_place = in_place;
    }
    if let Some(watch) = json::maybe_flag(params, "watch")? {
        opts.watch = watch;
    }
    if let Some(groups) = json::maybe_flag(params, "groups")? {
        opts.groups = groups;
    }
    Ok(LibraryAction::ImportFolder { root, opts })
}

/// The AI settings' jobs a script can send.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const JOBS: &str = "probe";

/// `ai.job`: the AI settings' background work, of which a script can probe.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn job(_at: &Session, params: &Value) -> Result<Command, String> {
    let name = json::text(params, "action")?;
    let job = match name.as_str() {
        "probe" => mareader_settings::Job::Probe,
        other => return Err(refused("ai.job", other, JOBS)),
    };
    Ok(Command::Ai(job))
}

/// Software update actions a script can send.
pub(crate) const UPDATE: &str =
    "check|install|restart|dismiss|show_modal|open_download|open_notes|mock";

/// `update.act`: checks for updates, stages/restarts, or mocks a release.
pub(crate) fn update(_at: &Session, params: &Value) -> Result<Command, String> {
    let name = json::text(params, "action")?;
    let act = match name.as_str() {
        "check" => UpdateAction::Check {
            manual: json::off_unless(params, "manual")?,
        },
        "install" => UpdateAction::Install,
        "restart" => UpdateAction::Restart,
        "dismiss" => UpdateAction::Dismiss,
        "show_modal" => UpdateAction::ShowModal,
        "open_download" => UpdateAction::OpenDownload,
        "open_notes" => UpdateAction::OpenNotes,
        "mock" => UpdateAction::Mock(mock_update(params)?),
        other => return Err(refused("update.act", other, UPDATE)),
    };
    Ok(Command::Update(act))
}

fn mock_update(params: &Value) -> Result<MockUpdate, String> {
    let version = json::maybe_text(params, "version")?.unwrap_or_else(|| "99.0.0".to_owned());
    let notes = json::maybe_text(params, "notes")?;
    let url = json::maybe_text(params, "url")?;
    let platform = match json::maybe_text(params, "platform")? {
        Some(raw) => Some(
            UpdatePlatform::parse(&raw)
                .ok_or_else(|| json::wants("platform", "mac|windows|linux"))?,
        ),
        None => None,
    };
    let mode = match json::maybe_text(params, "mode")? {
        Some(raw) => Some(
            MockMode::parse(&raw).ok_or_else(|| json::wants("mode", "notify|install|staged"))?,
        ),
        None => None,
    };
    Ok(MockUpdate {
        version,
        notes,
        url,
        platform,
        mode,
    })
}

/// An ask a script cannot send, beside the names it can send.
pub(super) fn refused(id: &str, name: &str, forms: &str) -> String {
    format!("{id} takes {forms} from a script; {name:?} is not one of them")
}
