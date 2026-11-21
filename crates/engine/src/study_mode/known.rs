//! Words already memorized, as a deck skips them.

use std::collections::HashSet;

use mareader_study::mode::Key;

/// Words already memorized; a key without a class matches every class.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Known {
    exact: HashSet<Key>,
    lemmas: HashSet<String>,
}

impl Known {
    pub fn of(keys: impl IntoIterator<Item = Key>) -> Self {
        let mut known = Self::default();
        for key in keys {
            known.add(key);
        }
        known
    }

    pub fn add(&mut self, key: Key) {
        self.lemmas.insert(key.lemma.clone());
        self.exact.insert(key);
    }

    pub fn has(&self, key: &Key) -> bool {
        let bare = || self.exact.contains(&Key::new(&*key.lemma, None));
        match key.pos {
            Some(_) => self.exact.contains(key) || bare(),
            None => self.lemmas.contains(&key.lemma),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use mareader_study::mode::Pos;

    #[test]
    fn a_key_without_a_class_matches_every_class() {
        let known = Known::of([Key::new("run", None), Key::new("swim", Some(Pos::Verb))]);
        assert!(known.has(&Key::new("run", Some(Pos::Verb))));
        assert!(known.has(&Key::new("swim", None)));
        assert!(known.has(&Key::new("swim", Some(Pos::Verb))));
        assert!(!known.has(&Key::new("swim", Some(Pos::Noun))));
        assert!(!known.has(&Key::new("fly", None)));
    }
}
