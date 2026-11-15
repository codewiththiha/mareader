//! Pages' text and links, read on demand and kept for pages in view.

use std::collections::HashMap;
use std::sync::Arc;

use mareader_session::{DocId, RenderError, TextLayer};

/// Text layers kept; a few screens of pages across every pane.
pub const MAX_PAGES: usize = 96;

/// A page's read text, or the note that reading it failed.
#[derive(Debug, Clone)]
enum Slot {
    Ready(Arc<TextLayer>),
    Failed,
}

/// Least recently shown layers go first once over the cap.
#[derive(Debug, Default)]
pub struct TextLayers {
    slots: HashMap<(DocId, usize), (Slot, u64)>,
    frame: u64,
}

impl TextLayers {
    pub fn get(&self, doc: DocId, page: usize) -> Option<Arc<TextLayer>> {
        match self.slots.get(&(doc, page)) {
            Some((Slot::Ready(layer), _)) => Some(Arc::clone(layer)),
            _ => None,
        }
    }

    /// Marks a page in view; false if it still needs reading.
    pub fn touch(&mut self, doc: DocId, page: usize) -> bool {
        let frame = self.frame;
        self.slots
            .get_mut(&(doc, page))
            .map(|(_, used)| *used = frame)
            .is_some()
    }

    pub fn insert(&mut self, doc: DocId, page: usize, result: Result<Arc<TextLayer>, RenderError>) {
        let slot = result.map_or(Slot::Failed, Slot::Ready);
        self.slots.insert((doc, page), (slot, self.frame));
    }

    /// Hands `old`'s layers to `new` where `map` finds the same page.
    pub fn adopt(&mut self, old: DocId, new: DocId, map: impl Fn(usize) -> Option<usize>) {
        let pages: Vec<usize> = (self.slots.keys())
            .filter(|(d, _)| *d == old)
            .map(|(_, p)| *p)
            .collect();
        for page in pages {
            let Some(to) = map(page).filter(|to| !self.slots.contains_key(&(new, *to))) else {
                continue;
            };
            if let Some(slot) = self.slots.remove(&(old, page)) {
                self.slots.insert((new, to), slot);
            }
        }
        self.forget(old);
    }

    pub fn forget(&mut self, doc: DocId) {
        self.slots.retain(|(d, _), _| *d != doc);
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// At rest: only the layers of pages in view this frame stay.
    pub fn rest(&mut self) {
        let frame = self.frame;
        self.slots.retain(|_, (_, used)| *used == frame);
    }

    /// Drops the stalest layers past the cap.
    pub fn end_frame(&mut self) {
        self.frame += 1;
        let over = self.slots.len().saturating_sub(MAX_PAGES);
        if over == 0 {
            return;
        }
        let mut ages: Vec<_> = self
            .slots
            .iter()
            .map(|(k, (_, used))| (*used, *k))
            .collect();
        ages.sort_unstable();
        for (_, key) in ages.into_iter().take(over) {
            self.slots.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_least_recently_shown_pages_are_dropped_past_the_cap() -> Result<(), &'static str> {
        let doc = crate::sample::doc()?;
        let mut texts = TextLayers::default();
        for page in 0..MAX_PAGES {
            texts.insert(doc, page, Ok(Arc::default()));
        }
        texts.end_frame();
        // Page 0 is still on screen; two new pages arrive.
        assert!(texts.touch(doc, 0));
        texts.insert(doc, 500, Ok(Arc::default()));
        texts.insert(doc, 501, Err(RenderError::Failed));
        texts.end_frame();
        assert_eq!(texts.len(), MAX_PAGES);
        assert!(texts.get(doc, 0).is_some() && texts.get(doc, 500).is_some());
        assert!(!texts.touch(doc, 1) && !texts.touch(doc, 2));
        // A failed read counts as read, so it isn't asked for again.
        assert!(texts.touch(doc, 501) && texts.get(doc, 501).is_none());
        texts.forget(doc);
        assert!(texts.is_empty());
        Ok(())
    }

    #[test]
    fn at_rest_only_the_pages_in_view_keep_their_text() -> Result<(), &'static str> {
        let doc = crate::sample::doc()?;
        let mut texts = TextLayers::default();
        for page in 0..10 {
            texts.insert(doc, page, Ok(Arc::default()));
        }
        texts.end_frame();
        assert!(texts.touch(doc, 4));
        texts.rest();
        assert_eq!(texts.len(), 1);
        assert!(texts.get(doc, 4).is_some());
        Ok(())
    }
}
