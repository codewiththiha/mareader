//! A study state's marks restyled, and their notes written and deleted.

use mareader_library::{Library, Mark, MarkId, Style};

use super::State;
use crate::command::Command;
use crate::study::Action;
use crate::toast::Toast;

impl State {
    /// Updates a single highlight's style.
    pub fn restyle(&mut self, main: &mut Library, id: MarkId, style: Style, wall: u64) {
        if !main.set_style(id, style, wall) {
            self.loose.set_style(id, style, wall);
        }
    }

    /// Restyles picked highlights; returns an Undo toast when any changed.
    pub(crate) fn restyle_all(
        &mut self,
        main: &mut Library,
        changes: Vec<(MarkId, Style)>,
        skipped: usize,
        wall: u64,
    ) -> Toast {
        let mut before = Vec::new();
        for (id, style) in changes {
            let Some(was) = self.mark(main, id).map(Mark::look) else {
                continue;
            };
            if main.set_style(id, style, wall) || self.loose.set_style(id, style, wall) {
                before.push((id, was));
            }
        }
        let n = before.len();
        let message = match (n, skipped) {
            (0, _) => "That look is kept for marks of their own".to_owned(),
            (1, 0) => "Restyled 1 highlight".to_owned(),
            (n, 0) => format!("Restyled {n} highlights"),
            (n, s) => format!("Restyled {n} of {} highlights", n + s),
        };
        match n {
            0 => Toast::info(message),
            _ => Toast::info(message).undo(Command::Study(Action::Unstyle(before))),
        }
    }

    /// Reverts picked highlights' previous styles (Undo).
    pub(crate) fn unstyle(&mut self, main: &mut Library, looks: &[(MarkId, Style)], wall: u64) {
        for &(id, style) in looks {
            if !main.set_style(id, style, wall) {
                self.loose.set_style(id, style, wall);
            }
        }
    }

    /// Saves note text on mark `id`.
    pub fn set_note(&mut self, main: &mut Library, id: MarkId, text: String, wall: u64) {
        if !main.set_note(id, Some(text.clone()), wall) {
            self.loose.set_note(id, Some(text), wall);
        }
    }

    /// Deletes `id`'s note, returning an Undo toast when one was present.
    pub(crate) fn forget_note(
        &mut self,
        main: &mut Library,
        id: MarkId,
        wall: u64,
    ) -> Option<Toast> {
        let was = self.mark(main, id).and_then(|m| m.note.value.clone());
        if !main.set_note(id, None, wall) {
            self.loose.set_note(id, None, wall);
        }
        was.map(|text| {
            let undo = Command::Study(Action::RestoreNote(id, text));
            Toast::info("Deleted note").undo(undo)
        })
    }

    /// Restores a deleted note's text (Undo).
    pub(crate) fn restore_note(&mut self, main: &mut Library, id: MarkId, text: String, wall: u64) {
        if !main.set_note(id, Some(text.clone()), wall) {
            self.loose.set_note(id, Some(text), wall);
        }
    }
}
