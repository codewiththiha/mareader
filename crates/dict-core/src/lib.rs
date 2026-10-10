//! Dictionary core: POS canon, ranks, bridge plans, fuzzy search.

pub mod bridge;
pub mod entry;
pub mod lang;
pub mod pos;
pub mod search;

pub use bridge::{HUB, Hop, PackRef, Plan, plans};
pub use entry::{DictEntry, EntryRank, WordMatch, order_entries};
pub use lang::{Script, detect, name, script, speakers};
pub use pos::{CanonPos, PosMatch, canonize, kind_canon, parse_tags, penn_canon, rank_tags};
pub use search::{classify, edit_distance, fold};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_surface_stays_small() {
        // The menu only ever needs: parse, rank, order, plan, search.
        let tags = parse_tags("v,n");
        let mut rows = vec![];
        order_entries(&mut rows, Some(CanonPos::Verb));
        let _ = plans("en", "fr", &[]);
        let _ = classify("word", "word");
        let _ = EntryRank::of(
            &DictEntry {
                word: "w".into(),
                pos_raw: None,
                tags,
                definition: "d".into(),
                romanization: None,
                sense: None,
                pack: "p".into(),
                via: None,
                word_match: WordMatch::Exact,
            },
            None,
        );
    }
}
