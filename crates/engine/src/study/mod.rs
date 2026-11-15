//! What the word card and the mark menu ask of the app.

use mareader_library::{MarkId, Style};
use mareader_workspace::PaneId;

pub mod asking;
pub mod loose;
pub mod state;
pub mod words;

/// What the card and the mark menu ask of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Remove(Vec<MarkId>),
    /// Undo: the removed marks come back.
    Restore(Vec<MarkId>),
    /// Asks again after a failure.
    Retry,
    /// Asks the AI chosen now; its answer replaces the saved one.
    AskAgain,
    Close,
    /// The mark menu's Select several: picking begins with its mark.
    SelectSeveral(PaneId, MarkId),
    /// Picks the marks on screen, or every mark of the book.
    PickPage,
    PickAll,
    DonePicking,
    /// The mark menu's Add note or Edit note.
    WriteNote(MarkId),
    /// The notes panel's: a draft in `PaneId`, placed once its words show.
    WriteNoteIn(PaneId, MarkId),
    /// The open note's Edit: its text becomes a draft.
    EditNote,
    /// Keeps the draft on `MarkId`'s note; blank deletes it.
    SaveNote(MarkId),
    CancelNote,
    DeleteNote(MarkId),
    /// Undo: a deleted note's text comes back.
    RestoreNote(MarkId, String),
    /// A highlight's new pen and colour, from its menu.
    Restyle(MarkId, Style),
    /// Picked highlights' new looks, and how many couldn't take it.
    RestyleAll(Vec<(MarkId, Style)>, usize),
    /// Undo: the picked highlights' looks before.
    Unstyle(Vec<(MarkId, Style)>),
    /// Shots: types into the open draft, then saves it if asked.
    TypeNote(String, bool),
    /// Writes note text directly onto a mark.
    SetNote(MarkId, String),
    /// Drops placeholder marks quietly without an undo toast.
    DropMarks(Vec<MarkId>),
}
