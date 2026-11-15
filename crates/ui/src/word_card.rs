//! Small state operations shared by the dictionary and study word cards.

use mareader_cefr::Role;

pub(crate) trait Card {
    fn word(&self) -> &str;
    fn set_role(&mut self, role: Role);
}

macro_rules! impl_card {
    ($type:ty) => {
        impl crate::word_card::Card for $type {
            fn word(&self) -> &str {
                &self.word
            }

            fn set_role(&mut self, role: Role) {
                self.role = Some(role);
            }
        }
    };
}
pub(crate) use impl_card;

/// Set the role only when the answer still belongs to the open card's word.
pub(crate) fn set_role<C: Card>(card: &mut Option<C>, word: &str, role: Role) {
    if let Some(card) = card.as_mut().filter(|card| card.word() == word) {
        card.set_role(role);
    }
}
