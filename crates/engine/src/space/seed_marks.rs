//! Made-up highlights, notes and bookmarks in the first books, so shots and
//! tests fill Space's notes.

use mareader_library::{BookId, Format, Library, NewMark, Pen, Spot};

const HOUR_MS: u64 = 3_600_000;

/// The book it is in, the words, the pen, a picked colour, and a note.
type Seed = (
    usize,
    &'static str,
    Pen,
    Option<[u8; 3]>,
    Option<&'static str>,
);

/// Oldest first; the book is taken in turn from the first three.
const MARKS: [Seed; 9] = [
    (
        0,
        "It was the best of times, it was the worst of times.",
        Pen::Full,
        None,
        None,
    ),
    (
        0,
        "We had everything before us, we had nothing before us.",
        Pen::Underline,
        None,
        Some("The pairs keep cancelling out. Look for this again in book two."),
    ),
    (
        1,
        "The mind is its own place.",
        Pen::Full,
        Some([0x83, 0xd3, 0x3c]),
        None,
    ),
    (
        1,
        "Long is the way and hard, that out of hell leads up to light.",
        Pen::Half,
        Some([0x00, 0xb2, 0xf4]),
        Some("Compare with the opening of the Inferno."),
    ),
    (2, "Chapter 3", Pen::Full, None, None),
    (
        2,
        "A truth universally acknowledged.",
        Pen::Full,
        Some([0xa5, 0x8a, 0xf0]),
        Some("Irony from the first line: the narrator means the opposite."),
    ),
    (
        0,
        "a quiet sentence about the river",
        Pen::Half,
        Some([0xff, 0xbe, 0x00]),
        None,
    ),
    (1, "Chapter 9", Pen::Full, None, None),
    (
        0,
        "Nothing that we despise in other men is inherently absurd.",
        Pen::Full,
        None,
        Some("Ask about this in class."),
    ),
];

/// Marks the first three books; how many were kept. "Chapter" words make
/// bookmarks. With no book there is nowhere to keep them.
pub(crate) fn seed_marks(model: &mut Library, now_ms: u64) -> usize {
    let books: Vec<(BookId, Format)> = model.books().take(3).map(|b| (b.id, b.format)).collect();
    let mut kept = 0;
    let last = MARKS.len() as u64;
    for (i, &(nth, words, pen, color, note)) in MARKS.iter().enumerate() {
        let Some(&(book, format)) = books.get(nth % books.len().max(1)) else {
            break;
        };
        let spot = spot(format, i);
        let new = match words.starts_with("Chapter") {
            true => NewMark::bookmark(words.to_owned(), spot),
            false => NewMark::highlight(words.to_owned(), spot, pen, color),
        };
        let at = now_ms.saturating_sub((last - i as u64) * 20 * HOUR_MS);
        let Some(id) = model.add_mark(book, new, at) else {
            continue;
        };
        kept += 1;
        if let Some(note) = note {
            model.set_note(id, Some(note.to_owned()), at + 1);
        }
    }
    kept
}

/// The `i`th mark's place: a page in a PDF, else a block of text.
fn spot(format: Format, i: usize) -> Spot {
    match format {
        Format::Pdf => Spot::Page {
            page: 2 + 3 * i,
            rect: [0.1, 0.2, 0.8, 0.24],
        },
        _ => Spot::Text {
            block: 4 + 6 * i,
            start: 0,
            end: 24,
        },
    }
}
