//! The AI dictionary inside [`Session`]: Explain answers filed, and the
//! card's offer to replace a kept meaning.

use crate::ai_dict::of_said;
use crate::study::state::Key;

use super::Session;

impl Session {
    /// Files the Explain answers that came, when an AI wrote them.
    pub(crate) fn study_file(&mut self) {
        let filed = self.study.take_filed();
        let Some(language) = self.study.language() else {
            return;
        };
        for (key, said) in filed {
            if let Some(saved) = of_said(&said, language) {
                let ticket = self.dict.remember(vec![saved.clone()], false);
                self.study.ai_dict.sent(ticket, key, saved);
            }
        }
    }

    /// Takes in filing replies; true when a card gained an offer.
    pub(crate) fn study_offers(&mut self) -> bool {
        let dict = &mut self.dict;
        self.study.ai_dict.collect(|ticket| dict.take_filed(ticket))
    }

    /// The card's Replace: its answer takes the kept meaning's place.
    pub fn study_replace(&mut self, key: Key) {
        if let Some(offer) = self.study.ai_dict.take_offer(key) {
            self.dict.remember(vec![offer.new], true);
        }
    }
}
