//! The open word card's window state: placement, role, and opening.

use egui::Rect;
use mareader_cefr::Role;
use mareader_engine::session::Session;
use mareader_library::{BookId, MarkId};
use mareader_workspace::PaneId;

use super::{Card, Explained, StudyUi};
use crate::word_card::TabSnapshot;
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
        self.stash = None;
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

    /// Drops any stashed AI card.
    pub fn clear_stash(&mut self) {
        self.stash = None;
    }

    /// Stashes the open AI card so returning to AI in context is instant.
    pub fn stash_for_tab(&mut self) -> Option<TabSnapshot> {
        let card = self.card.take()?;
        let snap = TabSnapshot {
            pane: card.pane,
            word: card.word.clone(),
            context: card.context.clone(),
            asked: None,
            at: card.at,
            role: card.role.clone(),
            rect: self.card_rect,
        };
        self.stash = Some(card);
        Some(snap)
    }

    /// Opens the AI tab from a Dictionary card, restoring any stashed card.
    pub fn open_from_tab(
        &mut self,
        session: &mut Session,
        book: Option<BookId>,
        name: &str,
        snap: TabSnapshot,
        wall_ms: u64,
    ) {
        self.menu = None;
        self.note = None;
        let matches = |c: &Card| c.pane == snap.pane && c.word == snap.word && c.at == snap.at;
        if let Some(mut card) = self.stash.take().filter(matches) {
            if card.role.is_none() {
                card.role = snap.role;
            }
            card.seen = true;
            self.card_rect = snap.rect;
            self.card = Some(card);
            return;
        }
        let (key, word, context) = match snap.asked {
            Some(asked) => {
                let key = session.study_explain(book, name, &asked, wall_ms);
                (key, asked.word, asked.context)
            }
            None => {
                let key = session.study_explain_loose(&snap.word, &snap.context);
                (key, snap.word, snap.context)
            }
        };
        self.card_rect = snap.rect;
        self.card = Some(Card {
            pane: snap.pane,
            key,
            word,
            context,
            at: snap.at,
            seen: true,
            role: snap.role,
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
        self.stash = None;
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
