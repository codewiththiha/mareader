//! Small state operations shared by the dictionary and study word cards.

pub use mareader_cefr::Role;
use mareader_workspace::PaneId;

/// A word card that can take its word's role.
pub trait Card {
    fn pane(&self) -> PaneId;
    fn word(&self) -> &str;
    fn set_role(&mut self, role: Role);
}

/// Implements [`Card`] for a type with `pane`, `word` and `role` fields.
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
        }
    };
}

/// Set the role only when the answer still belongs to the open card's word.
pub fn set_role<C: Card>(card: &mut Option<C>, word: &str, role: Role) {
    if let Some(card) = card.as_mut().filter(|card| card.word() == word) {
        card.set_role(role);
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
