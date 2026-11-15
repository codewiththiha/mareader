//! The open word card's window state: placement, role, and opening.

use egui::Rect;
use mareader_cefr::Role;
use mareader_engine::session::Session;
use mareader_library::MarkId;
use mareader_workspace::PaneId;

use super::{Card, Explained, StudyUi};
use mareader_engine::study::state::Key;

impl StudyUi {
    /// The mark whose card or note is open in `pane`.
    pub fn card_mark(&self, pane: PaneId) -> Option<MarkId> {
        if let Some(n) = self.note.as_ref().filter(|n| n.pane == pane) {
            return Some(n.mark.id);
        }
        if let Some(c) = self.card.as_ref().filter(|c| c.pane == pane) {
            return match c.key {
                Key::Mark(id) => Some(id),
                Key::Loose(_) => None,
            };
        }
        self.menu.filter(|m| m.pane == pane).map(|m| m.mark)
    }

    pub fn card_open(&self) -> bool {
        self.card.is_some()
    }

    /// The word the open card explains.
    pub fn card_word(&self) -> Option<&str> {
        self.card.as_ref().map(|c| c.word.as_str())
    }

    /// The open card's word and context, to ask for its role.
    pub fn card_words(&self) -> Option<(&str, &str)> {
        let c = self.card.as_ref()?;
        Some((c.word.as_str(), c.context.as_str()))
    }

    /// The role the open card shows, for tests.
    pub fn card_role(&self) -> Option<&Role> {
        self.card.as_ref()?.role.as_ref()
    }

    /// The tagger's answer for the open card, if it is still `word`'s.
    pub fn set_role(&mut self, word: &str, role: Role) {
        mareader_engine::word_card::set_role(&mut self.card, word, role);
    }

    /// Opens the AI Dictionary card for a word already marked by the session.
    pub fn show_explained(
        &mut self,
        pane: PaneId,
        key: Key,
        word: String,
        context: String,
        at: Rect,
    ) {
        self.menu = None;
        self.note = None;
        self.card = Some(Card {
            pane,
            key,
            word,
            context,
            at,
            seen: true,
            role: None,
        });
    }

    /// Explain pressed: marks the word in the session and opens its card.
    pub fn explain(&mut self, session: &mut Session, at: Explained<'_>, wall_ms: u64) {
        let Explained {
            pane,
            book,
            name,
            asked,
            rect,
        } = at;
        let key = session.study_explain(book, name, &asked, wall_ms);
        self.show_explained(pane, key, asked.word, asked.context, rect);
    }

    pub(super) fn open(
        &mut self,
        session: &mut Session,
        pane: PaneId,
        key: Key,
        words: (String, String),
        at: Rect,
    ) {
        let (word, context) = words;
        self.menu = None;
        self.note = None;
        session.study_prepare_card(key, &word, &context);
        self.card = Some(Card {
            pane,
            key,
            word,
            context,
            at,
            seen: true,
            role: None,
        });
    }
}
