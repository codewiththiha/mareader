//! A selection as a word to explain: its text, context and spot.

use std::ops::Range;
use std::sync::Arc;

use mareader_library::Spot;
use mareader_pdf::Bounds;
use mareader_session::{Selection, TextLayer};

/// What Explain sends: the word, the words around it, where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    pub word: String,
    pub context: String,
    /// None where a mark can't be kept, as in a table.
    pub spot: Option<Spot>,
    pub page: usize,
    /// The word's characters on that page.
    pub chars: Range<usize>,
}

/// A page's text with gaps as spaces; each char's layer index, if any.
pub fn read(layer: &TextLayer) -> (Vec<char>, Vec<Option<usize>>) {
    let chars = layer.text.chars();
    let mut text = Vec::with_capacity(chars.len() + chars.len() / 4);
    let mut index = Vec::with_capacity(text.capacity());
    let mut line = None;
    for (i, c) in chars.iter().enumerate() {
        let broke = line.is_some_and(|l| l != c.line);
        if (broke || c.space_before) && !c.ch.is_whitespace() {
            text.push(' ');
            index.push(None);
        }
        text.push(c.ch);
        index.push(Some(i));
        line = Some(c.line);
    }
    (text, index)
}

/// The selected word, or none if the selection isn't one word on one page.
pub fn asked(
    selection: Selection,
    layer: impl Fn(usize) -> Option<Arc<TextLayer>>,
) -> Option<Asked> {
    let pages = selection.pages();
    if pages.len() != 1 {
        return None;
    }
    let page = pages.start;
    let layer = layer(page)?;
    let range = selection.on_page(page, layer.text.len())?;
    let (text, index) = read(&layer);
    let inside = |p: &Option<usize>| p.is_some_and(|i| range.contains(&i));
    let first = index.iter().position(inside)?;
    let last = index.iter().rposition(inside)?;
    let picked: String = text.get(first..=last)?.iter().collect();
    let word = mareader_study::word_in(&picked)?;
    let (from, to) = (first + word.start, first + word.end);
    let chars = index.get(from)?.as_ref().copied()?..index.get(to - 1)?.map(|i| i + 1)?;
    let full: String = text.iter().collect();
    Some(Asked {
        word: text.get(from..to)?.iter().collect(),
        context: mareader_study::context(&full, from..to),
        spot: spot(&layer, page, chars.clone()),
        page,
        chars,
    })
}

/// The word at `chars` of a page, as Explain asks for it.
pub fn asked_at(layer: &TextLayer, page: usize, chars: Range<usize>) -> Option<Asked> {
    let (text, index) = read(layer);
    let from = index.iter().position(|i| *i == Some(chars.start))?;
    let last = chars.end.checked_sub(1)?;
    let to = index.iter().rposition(|i| *i == Some(last))? + 1;
    let full: String = text.iter().collect();
    Some(Asked {
        word: text.get(from..to)?.iter().collect(),
        context: mareader_study::context(&full, from..to),
        spot: spot(layer, page, chars.clone()),
        page,
        chars,
    })
}

/// The characters `chars` of a page, as written.
pub fn text_of(layer: &TextLayer, chars: Range<usize>) -> String {
    let all = layer.text.chars();
    all.get(chars)
        .unwrap_or_default()
        .iter()
        .map(|c| c.ch)
        .collect()
}

/// Where `chars` of a page stand, as a mark keeps it.
fn spot(layer: &TextLayer, page: usize, chars: Range<usize>) -> Option<Spot> {
    if layer.places.is_empty() {
        let b = layer.text.line_boxes(chars).reduce(Bounds::union)?;
        let rect = [b.x0, b.y0, b.x1 - b.x0, b.y1 - b.y0];
        return Some(Spot::Page { page, rect });
    }
    let (block, start) = layer.place(chars.start)?;
    let last = chars.end.checked_sub(1)?;
    let (end_block, end) = layer.place(last)?;
    let width = layer.text.chars().get(last).map_or(1, |c| c.ch.len_utf8());
    (block == end_block).then_some(Spot::Text {
        block,
        start,
        end: end + width,
    })
}

/// The characters of `layer` a book's mark covers on this page.
pub fn find(layer: &TextLayer, block: usize, bytes: Range<usize>) -> Option<Range<usize>> {
    let within = |i: usize| {
        layer
            .place(i)
            .is_some_and(|(b, at)| b == block && bytes.contains(&at))
    };
    let start = (0..layer.places.len()).find(|&i| within(i))?;
    let end = (start..layer.places.len())
        .take_while(|&i| within(i))
        .last()?;
    Some(start..end + 1)
}

/// Where a highlight of `sel` is kept: chars on a PDF, bytes in a book.
pub fn span(sel: Selection, layer: impl Fn(usize) -> Option<Arc<TextLayer>>) -> Option<Spot> {
    let (a, b) = sel.ordered();
    if layer(a.page)?.places.is_empty() {
        let (from, to) = ([a.page, a.index], [b.page, b.index]);
        return Some(Spot::Chars { from, to });
    }
    // A book's first and last placed chars; gaps between blocks have none.
    let placed = |page: usize| {
        let l = layer(page)?;
        let range = sel.on_page(page, l.text.len())?;
        Some((l, range))
    };
    let start = (a.page..=b.page).find_map(|p| {
        let (l, range) = placed(p)?;
        range.into_iter().find_map(|i| l.place(i))
    })?;
    let (end, width) = (a.page..=b.page).rev().find_map(|p| {
        let (l, range) = placed(p)?;
        range.rev().find_map(|i| {
            let width = l.text.chars().get(i)?.ch.len_utf8();
            Some((l.place(i)?, width))
        })
    })?;
    let (from, to) = ([start.0, start.1], [end.0, end.1 + width]);
    Some(Spot::Blocks { from, to })
}

#[cfg(test)]
mod tests {
    use mareader_pdf::PageText;

    use super::*;

    fn layer(words: &str, places: bool) -> Arc<TextLayer> {
        let at = |i: usize| {
            let x0 = i as f32 * 10.0;
            Bounds {
                x0,
                y0: 0.0,
                x1: x0 + 10.0,
                y1: 20.0,
            }
        };
        let line = words.chars().enumerate().map(|(i, c)| (c, at(i)));
        let places = match places {
            true => (0..words.chars().count())
                .map(|i| Some((4, 100 + i as u32)))
                .collect(),
            false => Vec::new(),
        };
        Arc::new(TextLayer {
            text: PageText::from_lines([(false, line.collect())]),
            places,
            ..TextLayer::default()
        })
    }

    #[test]
    fn a_selected_word_sheds_its_punctuation_and_keeps_its_context() {
        let page = layer("The quick, brown fox", false);
        let sel = Selection::within(0, 4..10);
        let got = asked(sel, |_| Some(Arc::clone(&page))).unwrap();
        assert_eq!(got.word, "quick");
        assert_eq!(got.chars, 4..9);
        assert_eq!(got.context, "The quick, brown fox");
        assert_eq!(
            got.spot,
            Some(Spot::Page {
                page: 0,
                rect: [40.0, 0.0, 50.0, 20.0]
            })
        );
        // Two words, or two pages, are not one word.
        let two = Selection::within(0, 4..16);
        assert_eq!(asked(two, |_| Some(Arc::clone(&page))), None);
    }

    #[test]
    fn a_book_s_word_is_kept_by_block_and_bytes_and_found_again() {
        let page = layer("The quick fox", true);
        let got = asked(Selection::within(0, 4..9), |_| Some(Arc::clone(&page))).unwrap();
        let spot = Spot::Text {
            block: 4,
            start: 104,
            end: 109,
        };
        assert_eq!(got.spot, Some(spot));
        assert_eq!(find(&page, 4, 104..109), Some(4..9));
        assert_eq!(find(&page, 5, 104..109), None);
    }

    #[test]
    fn a_highlight_spans_pages_by_char_on_a_pdf_and_by_byte_in_a_book() {
        use mareader_session::Caret;
        let pdf = layer("The quick fox", false);
        let over = Selection {
            anchor: Caret { page: 2, index: 9 },
            focus: Caret { page: 1, index: 4 },
        };
        let got = span(over, |_| Some(Arc::clone(&pdf)));
        let (from, to) = ([1, 4], [2, 9]);
        assert_eq!(got, Some(Spot::Chars { from, to }));
        let book = layer("The quick fox", true);
        let got = span(Selection::within(0, 4..13), |_| Some(Arc::clone(&book)));
        let (from, to) = ([4, 104], [4, 113]);
        assert_eq!(got, Some(Spot::Blocks { from, to }));
    }
}
