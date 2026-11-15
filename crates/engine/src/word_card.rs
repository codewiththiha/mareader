//! Small state operations shared by the dictionary and study word cards.

pub use mareader_cefr::Role;

/// A word card that can take its word's role.
pub trait Card {
    fn word(&self) -> &str;
    fn set_role(&mut self, role: Role);
}

/// Implements [`Card`] for a type with `word` and `role` fields.
#[macro_export]
macro_rules! impl_card {
    ($type:ty) => {
        impl $crate::word_card::Card for $type {
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
