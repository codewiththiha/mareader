//! Removal of selected words from the AI dictionary.

use super::{Reply, Shelf};

impl Shelf {
    pub(super) fn forget_ai_words(&mut self, words: &[(String, Option<String>)]) -> Reply {
        let mut reply = Reply::new(0);
        if let Err(why) = self.ai().and_then(|ai| ai.forget_words(words)) {
            reply.failed = Some(why);
        }
        reply.ai_langs = Some(self.ai_langs());
        reply
    }

}
