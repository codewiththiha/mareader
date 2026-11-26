//! Keeping selection answers in the library.

use mareader_library::{Origin, Spot};

use crate::event::Event;
use crate::library::{Status, wall_ms};
use crate::skills::{Anchor, Answer, AskId};
use crate::toast::Toast;

use super::Session;

impl Session {
    /// Toggle a selection's pin; defaults stay quiet if the book is loose.
    pub(super) fn skill_pin(&mut self, id: AskId, tell: bool) -> bool {
        if tell {
            self.library.retry_selection_writes();
        }
        if tell && let Some(ask) = self.skills.get_mut(id) {
            ask.auto_pin = false;
        }
        let Some(ask) = self.skills.get(id) else {
            return false;
        };
        if let Some(saved) = ask.pinned {
            self.library
                .model_mut()
                .set_ai_selection_alive(saved, false, wall_ms());
            if let Some(ask) = self.skills.get_mut(id) {
                ask.pinned = None;
            }
            return true;
        }
        if !matches!(ask.anchor, Anchor::Text(_)) {
            return false;
        }
        let book = ask.book.or_else(|| self.book_in(ask.pane));
        let in_library = book
            .and_then(|id| self.library.model().book(id))
            .is_some_and(|book| book.locations.iter().any(|l| l.origin != Origin::Opened));
        let ready = self.library.status == Status::Ready && self.library.folder.is_some();
        let record = ask
            .record()
            .filter(|r| matches!(r.spot, Spot::Chars { .. } | Spot::Blocks { .. }));
        if !in_library || !ready || record.is_none() {
            if tell {
                let message = if !in_library {
                    "Add this file to your library before pinning its answers."
                } else {
                    "This answer can't be saved here right now."
                };
                self.emit(Event::Toast(Toast::warning(message)));
            }
            return false;
        }
        let Some((book, record)) = book.zip(record) else {
            return false;
        };
        let saved = self
            .library
            .model_mut()
            .add_ai_selection(book, record, wall_ms());
        if let Some(saved) = saved {
            self.library.selection_pending(saved);
        }
        if let Some(ask) = self.skills.get_mut(id) {
            ask.book = Some(book);
            ask.pinned = saved;
            ask.pin_status = crate::library::SaveStatus::Pending;
        }
        saved.is_some()
    }

    /// A successful retry replaces the old saved answer, not a failed retry.
    pub(super) fn skill_save_landed(&mut self, id: AskId) {
        let Some(ask) = self.skills.get(id) else {
            return;
        };
        let (Some(old), Some(book), Answer::Ready(_), Some(record)) =
            (ask.pinned, ask.book, &ask.answer, ask.record())
        else {
            return;
        };
        let now = wall_ms();
        let saved = self.library.model_mut().add_ai_selection(book, record, now);
        if let Some(saved) = saved {
            self.library.selection_pending(saved);
            self.library
                .model_mut()
                .set_ai_selection_alive(old, false, now);
            if let Some(ask) = self.skills.get_mut(id) {
                ask.pinned = Some(saved);
                ask.pin_status = crate::library::SaveStatus::Pending;
            }
        }
    }

    pub(super) fn skill_cancel(&mut self, id: AskId) -> bool {
        let saved = self
            .skills
            .get(id)
            .and_then(|a| a.pinned)
            .and_then(|id| self.library.model().ai_selection(id))
            .filter(|s| !s.pending)
            .map(|s| (s.result.clone(), s.by.clone()));
        match saved {
            Some((text, by)) => self.skills.cancel_to(id, text, by),
            None => self.skill_remove(id),
        }
    }

    pub(super) fn skill_remove(&mut self, id: AskId) -> bool {
        self.library.retry_selection_writes();
        if let Some(saved) = self.skills.get(id).and_then(|a| a.pinned) {
            self.library
                .model_mut()
                .set_ai_selection_alive(saved, false, wall_ms());
        }
        self.skills.remove(id)
    }
}
