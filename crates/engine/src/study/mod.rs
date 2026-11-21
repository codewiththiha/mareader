//! What the word card and the mark menu ask of the app.

use mareader_library::{MarkId, Style};
use mareader_study::StudyMode;
use mareader_workspace::PaneId;

pub mod asking;
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
pub mod mode;
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
    /// Turns study mode on or off.
    Mode(StudyMode),
    /// Marks a studied word memorized, globally across books.
    MarkKnown {
        key: String,
    },
    /// Puts a memorized word back into rotation.
    Unmemorize {
        key: String,
    },
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
