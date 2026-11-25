//! Made-up studied words in the first books, so shots and tests fill Space.

use mareader_library::{BookId, LearnedBy, Library, MeaningBy, NewWord, WordStatus};

const DAY_MS: u64 = 86_400_000;
/// Taken in turn, so every tab has words.
const STATUSES: [WordStatus; 3] = [
    WordStatus::Memorized,
    WordStatus::Learning,
    WordStatus::Missed,
];

/// Lemma, class, band, the book's sentence and the meaning.
const WORDS: [(&str, &str, u8, &str, &str); 8] = [
    (
        "lintel",
        "noun",
        5,
        "He ducked beneath the lintel of the low door.",
        "a beam across the top of a door or window",
    ),
    (
        "gaunt",
        "adjective",
        4,
        "The gaunt hound watched from the porch.",
        "very thin, as from hunger or age",
    ),
    (
        "ponder",
        "verb",
        3,
        "She paused to ponder the letter once more.",
        "to think about something carefully",
    ),
    (
        "arcane",
        "adjective",
        6,
        "The rules of the guild were arcane to outsiders.",
        "known or understood by very few",
    ),
    (
        "brusquely",
        "adverb",
        5,
        "He answered brusquely and turned away.",
        "in a short, abrupt and rude way",
    ),
    (
        "tenuous",
        "adjective",
        5,
        "Their claim to the house was tenuous at best.",
        "very weak or slight",
    ),
    (
        "forage",
        "verb",
        4,
        "Crows forage along the river after rain.",
        "to search widely for food",
    ),
    (
        "reverie",
        "noun",
        6,
        "A knock on the door broke her reverie.",
        "a state of pleasant, dreamy thought",
    ),
];

/// Who wrote the meanings, in turn; older rows don't say.
const WRITERS: [Option<MeaningBy>; 3] = [Some(MeaningBy::Dictionary), Some(MeaningBy::Ai), None];

/// Studies the words in the first three books; how many were kept.
///
/// Statuses take turns, every other word was recalled, and the first two
/// are met again in a second book, so every tab, scope and stat has rows.
/// With no book there is nowhere to keep them.
pub(crate) fn seed(model: &mut Library, now_ms: u64) -> usize {
    let books: Vec<BookId> = model.books().take(3).map(|b| b.id).collect();
    let mut kept = 0;
    for (i, &(lemma, pos, band, context, meaning)) in WORDS.iter().enumerate() {
        let recalled = i % 2 == 1;
        let new = NewWord {
            lemma: lemma.to_owned(),
            pos: Some(pos.to_owned()),
            band,
            form: lemma.to_owned(),
            context: context.to_owned(),
            meaning: meaning.to_owned(),
            meaning_by: WRITERS.get(i % WRITERS.len()).copied().flatten(),
            by: if recalled {
                LearnedBy::Reactive
            } else {
                LearnedBy::Proactive
            },
            status: STATUSES
                .get(i % STATUSES.len())
                .copied()
                .unwrap_or_default(),
            tries: if recalled { 1 + (i % 3) as u8 } else { 0 },
            first_try: recalled && i.is_multiple_of(3),
        };
        let at = now_ms.saturating_sub(i as u64 * 9 * DAY_MS);
        let nth = |n: usize| books.get(n % books.len().max(1)).copied();
        let again = if i < 2 { nth(i + 1) } else { None };
        for book in [nth(i), again].into_iter().flatten() {
            kept += usize::from(model.record_word(book, new.clone(), at).is_some());
        }
    }
    kept
}
