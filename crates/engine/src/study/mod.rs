//! What the word card and the mark menu ask of the app.

use mareader_library::{MarkId, Style};
use mareader_workspace::PaneId;

pub mod asking;
pub mod candidates;
#[cfg(test)]
#[path = "mode_tests.rs"]
mod mode_tests;
/// A document outside the library, known by its name for the session.
pub(crate) mod loose {
    use std::path::PathBuf;

    use mareader_library::{BookId, ContentId, Format, Library, Measure, Opened};

    use crate::stable_hash::hash;

    /// Records session document `name` in `loose`, where its marks are kept.
    pub fn loose_book(loose: &mut Library, name: &str) -> BookId {
        let opened = Opened {
            path: PathBuf::from(format!("session:{name}")),
            measure: Measure {
                content: loose_content(name),
                stamp: Default::default(),
            },
            format: Format::Pdf,
            title: name.to_owned(),
            author: None,
        };
        loose.record_open(opened, 0)
    }

    /// Session document `name`'s id in a loose library.
    pub fn loose_id(name: &str) -> BookId {
        BookId::original(loose_content(name))
    }

    /// A document outside the library, known by its name for the session.
    pub fn loose_content(name: &str) -> ContentId {
        ContentId(u128::from(hash(name)))
    }
}
pub mod state;
pub mod words;

/// A word to record from a deck or a recall, in one book.
#[derive(Clone, Debug, PartialEq)]
pub struct WordRecord {
    pub lemma: String,
    /// A coarse class label: noun, verb, adjective, adverb or other.
    pub class: String,
    pub band: u8,
    pub meaning: String,
    pub context: String,
    pub spot: mareader_library::Spot,
    pub by: mareader_library::LearnedBy,
    pub status: mareader_library::WordStatus,
    pub attempts: u8,
    pub first_try: bool,
}

/// What the card and the mark menu ask of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Turns study mode on or off in a pane.
    SetMode(PaneId, mareader_study::StudyMode),
    /// Records a deck or recall word in the pane's book.
    RecordWord(PaneId, WordRecord),
    /// Sets a recorded word's status, as Space does.
    SetWordStatus(mareader_library::StudyWordId, mareader_library::WordStatus),
    /// Forgets recorded words, as Space does.
    ForgetWords(Vec<mareader_library::StudyWordId>),
    /// Unlocks a book up to a spot, as a finished deck does.
    Unlock(PaneId, mareader_library::Spot),
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
