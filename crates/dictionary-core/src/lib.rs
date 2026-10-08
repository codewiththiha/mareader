//! The dictionary's core: tags folded to kinds, keys folded for
//! lookup. No storage, no DOM.

pub mod key;
pub mod pos;

pub use key::{fold_key, query_keys};
pub use pos::{Kind, Match, agrees, display_tag, kind_of, kinds_of, rank, split_tags};
pub use pos::{kind_of_nlp, kinds_field};
