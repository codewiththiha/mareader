//! A saved selection answer for Space's example and UI tests.

use mareader_library::{BookId, Library, NewAiSelection, Spot};

pub(crate) fn seed_ai_notes(lib: &mut Library, now: u64) {
    let Some(book) = lib.books().next().map(|book| book.id) else {
        return;
    };
    seed_ai_at(lib, book, Spot::Chars { from: [0, 0], to: [0, 5] }, "A selected passage".into(), now);
}

pub(crate) fn seed_ai_at(lib: &mut Library, book: BookId, spot: Spot, source: String, now: u64) {
    let result = "The passage suggests how a small detail can change the tone of a scene. ".repeat(12);
    let _ = lib.add_ai_selection(book, NewAiSelection {
        skill: "simplify".into(), name: "Simplify".into(), spot, source,
        language: None, result, pending: false, by: "Example".into(),
    }, now);
}
