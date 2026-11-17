//! What the page asked for: one router for the study tools' actions.

use egui::Rect;
use mareader_engine::session::Session;
use mareader_engine::study::Action;

use super::StudyUi;

impl StudyUi {
    /// Handles front-end study UI actions; marks change in `Session::execute`.
    pub fn act(&mut self, session: &mut Session, action: Action) {
        match action {
            Action::Remove(ids) => self.act_remove(ids),
            Action::Retry => {
                if let Some(card) = self.card.clone() {
                    session.study_retry(card.key, &card.word, &card.context);
                }
            }
            Action::AskAgain => {
                if let Some(card) = self.card.clone() {
                    session.study_ask_again(card.key, &card.word, &card.context);
                }
            }
            Action::Close => {
                self.card = None;
                self.stash = None;
                self.menu = None;
            }
            Action::SelectSeveral(pane, mark) => self.pick(pane, mark),
            Action::Restyle(id, style) => {
                // The menu stays, showing the new look.
                if let Some(m) = self.menu.as_mut().filter(|m| m.mark == id) {
                    m.style = Some(style);
                }
            }
            Action::PickPage | Action::PickAll => {
                if let Some(p) = &mut self.picking {
                    let more = match action {
                        Action::PickPage => p.page.clone(),
                        _ => p.all.clone(),
                    };
                    p.add(&more);
                }
            }
            Action::DonePicking => self.picking = None,
            Action::WriteNote(id) => {
                if let Some(m) = self.menu.take() {
                    let (state, model) = (session.study(), session.library_model());
                    self.open_note(state, model, m.pane, id, m.rect, true);
                }
            }
            Action::WriteNoteIn(pane, id) => {
                self.open_note(
                    session.study(),
                    session.library_model(),
                    pane,
                    id,
                    Rect::NOTHING,
                    true,
                );
            }
            Action::EditNote => {
                if let Some(n) = &mut self.note {
                    n.draft = Some(n.mark.note.value.clone().unwrap_or_default());
                    n.focus = 2;
                }
            }
            Action::SaveNote(id) => self.save_note(session, id),
            Action::CancelNote => self.cancel_note(session),
            Action::DeleteNote(id) => {
                if self.note.as_ref().is_some_and(|n| n.mark.id == id) {
                    self.note = None;
                }
            }
            Action::TypeNote(text, save) => {
                let Some(n) = self.note.as_mut() else {
                    return;
                };
                n.draft = Some(text);
                if save {
                    let id = n.mark.id;
                    self.save_note(session, id);
                }
            }
            Action::Restore(_)
            | Action::RestyleAll(..)
            | Action::Unstyle(_)
            | Action::RestoreNote(..)
            | Action::SetNote(..)
            | Action::DropMarks(_) => {}
        }
    }
}
