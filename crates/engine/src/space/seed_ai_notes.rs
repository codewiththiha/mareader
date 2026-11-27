//! A saved selection answer for Space's example and UI tests.

use mareader_library::{Library, NewAiSelection, Spot};

pub(crate) fn seed_ai_notes(lib: &mut Library, now: u64) {
    let Some(book) = lib.books().next().map(|book| book.id) else { return; };
    let result = "The passage suggests how a small detail can change the tone of a scene. ".repeat(12);
    let _ = lib.add_ai_selection(book, NewAiSelection {
        skill: "simplify".into(), name: "Simplify".into(),
        spot: Spot::Chars { from: [0, 0], to: [0, 5] },
        source: "A selected passage".into(), language: None,
        result, pending: false, by: "Example".into(),
    }, now);
}
