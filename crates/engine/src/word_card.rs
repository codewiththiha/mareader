//! Small state operations shared by the dictionary and study word cards.
//!
//! A card's class comes from [`class`] alone, the one rule decks and
//! recalls follow too ([`Classes`]).

pub use mareader_cefr::Role;
use mareader_study::Classes;
pub use mareader_study::Pos;
use mareader_workspace::PaneId;

/// A word card that can take its word's role and dictionary class.
pub trait Card {
    fn pane(&self) -> PaneId;
    fn word(&self) -> &str;
    fn set_role(&mut self, role: Role);
    fn set_class(&mut self, pos: Pos);
}

/// A word card's class: the tagger's, else an AI's answer, else the
/// dictionary's; `ai` is none on cards no AI answers.
pub fn class(ai: Option<&str>, role: Option<&Role>, dictionary: Option<Pos>) -> Option<Pos> {
    let heard = Classes {
        tagger: role.and_then(|r| Pos::of_tag(&r.pos)),
        ai: ai.and_then(Pos::read),
        dictionary,
    };
    heard.class()
}

/// Implements [`Card`] for a type with `pane`, `word`, `role` and
/// `dictionary` fields.
#[macro_export]
macro_rules! impl_card {
    ($type:ty) => {
        impl $crate::word_card::Card for $type {
            fn pane(&self) -> mareader_workspace::PaneId {
                self.pane
            }

            fn word(&self) -> &str {
                &self.word
            }

            fn set_role(&mut self, role: $crate::word_card::Role) {
                self.role = Some(role);
            }

            fn set_class(&mut self, pos: $crate::word_card::Pos) {
                self.dictionary = Some(pos);
            }
        }
    };
}

/// Set the role only when the answer still belongs to the open card's word.
pub fn set_role<C: Card>(card: &mut Option<C>, word: &str, role: Role) {
    if let Some(card) = card.as_mut().filter(|card| card.word() == word) {
        card.set_role(role);
    }
}

/// Set the dictionary's class only while the card is still `word`'s.
pub fn set_class<C: Card>(card: &mut Option<C>, word: &str, pos: Pos) {
    if let Some(card) = card.as_mut().filter(|card| card.word() == word) {
        card.set_class(pos);
    }
}

/// Returns the open card's pane and headword.
pub fn pane_word<C: Card>(card: &Option<C>) -> Option<(PaneId, &str)> {
    let card = card.as_ref()?;
    Some((card.pane(), card.word()))
}

/// True when `card` belongs to `(pane, word)`.
pub fn matches<C: Card>(card: &Option<C>, pane: PaneId, word: &str) -> bool {
    card.as_ref()
        .is_some_and(|c| c.pane() == pane && c.word() == word)
}
