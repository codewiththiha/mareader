//! Tab stashing, the card's role and class, probing, and query helpers
//! for `DictUi`.

use egui::{Context, Rect, Vec2};
use mareader_cefr::Role;
use mareader_dict::{Pair, Query, Ticket};
use mareader_study::Pos;
use mareader_workspace::PaneId;

use super::{Card, DictUi, card};
use crate::word_card;

impl DictUi {
    /// The word the card shows.
    pub fn card_word(&self) -> Option<&str> {
        self.card.as_ref().map(|c| c.word.as_str())
    }

    /// Whether the open card came from resting on a word, for tests.
    pub fn card_rested(&self) -> bool {
        self.card.as_ref().is_some_and(|c| c.rest)
    }

    /// Follow an explicit card's text; a missing layout is not a dismissal.
    pub(super) fn follow_anchor(
        &mut self,
        locate: impl Fn(PaneId, &mut word_card::TextAnchor) -> Option<Option<Rect>>,
    ) -> bool {
        let Some(card) = self.card.as_mut().filter(|c| !c.rest) else {
            return false;
        };
        let Some(anchor) = &mut card.anchor else {
            return false;
        };
        match locate(card.pane, anchor) {
            Some(Some(at)) => {
                card.at = at;
                false
            }
            Some(None) => true,
            None => false,
        }
    }

    /// The tagger's answer for the open card, if it is still `word`'s.
    pub fn set_role(&mut self, word: &str, role: Role) {
        mareader_engine::word_card::set_role(&mut self.card, word, role);
    }

    /// The role the open card shows, for tests.
    pub fn card_role(&self) -> Option<&Role> {
        self.card.as_ref()?.role.as_ref()
    }

    /// The dictionary's class for the open card, if it is still `word`'s.
    pub fn set_class(&mut self, word: &str, pos: Pos) {
        mareader_engine::word_card::set_class(&mut self.card, word, pos);
    }

    /// Drops any stashed lookup card.
    pub fn clear_stash(&mut self) {
        self.stash = None;
    }

    /// Number of entries currently on the open lookup card.
    pub fn card_entries(&self) -> usize {
        self.card
            .as_ref()
            .and_then(|c| c.reply.as_ref())
            .map_or(0, |r| r.entries.len())
    }

    /// Steps the open lookup card's pager by `delta` (`-1` or `+1`).
    pub fn step_page(&self, ctx: &Context, delta: i32) {
        if let Some(c) = &self.card {
            card::step(ctx, &c.word, self.card_entries(), delta);
        }
    }

    /// Pane and word of the open lookup card.
    pub fn card_pane_word(&self) -> Option<(PaneId, &str)> {
        mareader_engine::word_card::pane_word(&self.card)
    }

    /// True if a stashed lookup or probe already exists for `(pane, word)`.
    pub fn has_stash_for(&self, pane: PaneId, word: &str) -> bool {
        mareader_engine::word_card::matches(&self.stash, pane, word)
    }

    /// True if the stashed lookup for `(pane, word)` has at least one entry.
    pub fn has_stashed_entries(&self, pane: PaneId, word: &str) -> bool {
        self.stash.as_ref().is_some_and(|c| {
            c.pane == pane
                && c.word == word
                && c.reply.as_ref().is_some_and(|r| !r.entries.is_empty())
        })
    }

    /// Stores a background probe so an AI card knows if dict data exists.
    pub fn stash_probe(
        &mut self,
        pane: PaneId,
        word: String,
        ticket: Ticket,
        at: Rect,
        offset: Option<Vec2>,
    ) {
        self.stash = Some(Card {
            pane,
            word,
            context: String::new(),
            asked: None,
            anchor: None,
            at,
            offset,
            ticket,
            reply: None,
            rest: false,
            role: None,
            dictionary: None,
        });
    }

    /// Stashes the open lookup card so switching back to Dictionary is instant.
    pub fn stash_for_tab(&mut self) -> Option<word_card::TabSnapshot> {
        let mut card = self.card.take()?;
        card.rest = false;
        if card.anchor.is_none() {
            card.anchor = card.asked.as_ref().map(|a| word_card::TextAnchor {
                spans: vec![(a.page, a.chars.clone())],
                spot: a.spot,
                doc: None,
                reflow_source: None,
            });
        }
        let snap = word_card::TabSnapshot {
            pane: card.pane,
            word: card.word.clone(),
            context: card.context.clone(),
            asked: card.asked.clone(),
            anchor: card.anchor.clone(),
            at: card.at,
            role: card.role.clone(),
            dictionary: card.dictionary,
            rect: self.card_rect,
        };
        self.stash = Some(card);
        Some(snap)
    }

    /// Restores a stashed lookup card matching `snap`; false if none fits.
    pub fn restore_from_tab(
        &mut self,
        snap: &word_card::TabSnapshot,
        offset: Option<Vec2>,
    ) -> bool {
        let same = |c: &Card| c.pane == snap.pane && c.word == snap.word;
        let Some(mut card) = self.stash.take().filter(same) else {
            return false;
        };
        card.dictionary = card.dictionary.or(snap.dictionary);
        if card.role.is_none() {
            card.role = snap.role.clone();
        }
        card.anchor = card.anchor.or_else(|| snap.anchor.clone());
        if let Some(anchor) = &mut card.anchor
            && anchor.doc.is_none()
        {
            anchor.doc = snap.anchor.as_ref().and_then(|a| a.doc);
            anchor.reflow_source = snap.anchor.as_ref().and_then(|a| a.reflow_source.clone());
        }
        card.at = snap.at;
        card.offset = offset;
        self.card_rect = snap.rect;
        self.card = Some(card);
        true
    }

    /// Opens the Dictionary tab for `snap` with an already-submitted `ticket`.
    pub fn open_tab_lookup(
        &mut self,
        snap: word_card::TabSnapshot,
        ticket: Ticket,
        offset: Option<Vec2>,
    ) {
        self.card_rect = snap.rect;
        self.card = Some(Card {
            pane: snap.pane,
            word: snap.word,
            context: snap.context,
            asked: snap.asked,
            anchor: snap.anchor,
            at: snap.at,
            offset,
            ticket,
            reply: None,
            rest: false,
            role: snap.role,
            dictionary: snap.dictionary,
        });
    }

    /// Builds a reversed-pair search query for the open lookup card.
    pub fn swap_query(&self) -> Option<Query> {
        let card = self.card.as_ref()?;
        let reply = card.reply.as_ref()?;
        let (from, to) = (reply.from.as_ref()?, reply.to.as_ref()?);
        let pair = Pair {
            from: Some(to.clone()),
            to: Some(from.clone()),
        };
        Some(Query::Search {
            text: card.word.clone(),
            pair: Some(pair),
            pack: None,
        })
    }

    /// Builds a lookup query in another target language for the open card.
    pub fn try_lang_query(&self, to: &str) -> Option<Query> {
        let card = self.card.as_ref()?;
        Some(Query::LookUp {
            word: card.word.clone(),
            to: Some(to.to_owned()),
        })
    }

    /// The open card's lookup asked again, in the language it answered in.
    pub fn again_query(&self) -> Option<Query> {
        let card = self.card.as_ref()?;
        let to = card.reply.as_ref().and_then(|r| r.to.clone());
        Some(Query::LookUp {
            word: card.word.clone(),
            to,
        })
    }

    /// Updates the open lookup card's pending dictionary query ticket.
    pub fn set_card_ticket(&mut self, ticket: Ticket) {
        if let Some(card) = &mut self.card {
            card.ticket = ticket;
        }
    }
}
