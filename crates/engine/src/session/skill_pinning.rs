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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use mareader_library::disk::Folder;
    use mareader_settings::{PageTask, Skill};
    use mareader_study::skill::Scripted;
    use mareader_workspace::PaneId;
    use crate::host::Fake;
    use crate::skills::{Asked, Anchor};

    fn session() -> Session {
        Session::headless(Arc::new(Fake::default()))
    }

    fn ask(session: &mut Session, book: Option<mareader_library::BookId>) -> AskId {
        session.skills.set_runner(Arc::new(Scripted::Echo));
        session.skills.start(Asked {
            pane: PaneId(1), doc: 1,
            skill: Skill::Builtin(PageTask::Simplify), name: "Simplify".into(),
            anchor: Anchor::Text(vec![(0, 0..4)]),
            book, spot: Some(Spot::Chars { from: [0, 0], to: [0, 4] }),
            text: "text".into(), path: None, layout: None, auto_pin: false,
        })
    }

    #[test]
    fn pinning_requires_an_imported_book_and_keeps_the_last_good_answer() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = session();
        s.library.folder = Some(Folder::new(dir.path()));
        s.library.seed(1);
        let loose = s.library.model.books().next().unwrap().id;
        let loose_ask = ask(&mut s, Some(loose));
        assert!(!s.skill_pin(loose_ask, true));
        assert!(s.skills.get(loose_ask).unwrap().pinned.is_none());
        s.library.seed_copies(1);
        let imported = s.library.model.books().find(|b| b.locations.iter().any(|l| l.origin == Origin::Store)).unwrap().id;
        let id = ask(&mut s, Some(imported));
        assert!(s.skill_pin(id, true));
        let pending = s.skills.get(id).unwrap().pinned.unwrap();
        assert!(s.library.model.ai_selection(pending).unwrap().pending);
        s.skills.receive();
        s.skill_save_landed(id);
        let saved = s.skills.get(id).unwrap().pinned.unwrap();
        assert_ne!(saved, pending);
        assert_eq!(s.library.model.ai_selection(saved).unwrap().result, "text");
        s.skills.set_runner(Arc::new(Scripted::Failing));
        assert!(s.skills.retry(id));
        s.skills.receive();
        s.skill_save_landed(id);
        assert_eq!(s.skills.get(id).unwrap().pinned, Some(saved));
        assert!(s.skill_pin(id, true));
        assert!(s.skills.get(id).unwrap().pinned.is_none());
        assert!(s.skills.get(id).is_some());
        assert!(s.library.model.ai_selection(saved).is_none());
    }
}
