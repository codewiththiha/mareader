//! Where a reflowable document's characters are, in the DOM.
//!
//! A page of pixels has a fixed grid, so anything painted over it — a gloss
//! stroke, a search hit — is placed by arithmetic on a stored rect. A document
//! the reader lays out itself has no grid: its blocks are re-cut by every
//! typography knob, and the only authority on where a word ended up is the
//! browser's own layout. This module is the one place that asks.
//!
//! The unit of address is the CHARACTER (a Unicode code point) counted over the
//! block row's text nodes in document order — what a gloss mark stores its spot
//! in, and what a search hit's occurrence ordinal counts. The DOM's UTF-16 code
//! units are converted to at the `set_start`/`set_end` boundary and nowhere
//! else, so an emoji is one character on both sides of that line.
//!
//! Two callers, one walk:
//!
//! * [`crate::components::ai::reflow_anchor`] projects a persisted mark's spot
//!   back to pixels, once per mark per refresh;
//! * [`crate::components::formats::reflow::highlight`] covers a block's row in
//!   search hits, once per row per invalidation.
//!
//! Both need the same two things — the row's text nodes, and a `Range` over a
//! span of them — and both need the walk to skip the layers painted OVER the
//! text rather than being part of it. Measuring a `Range` is not this module's
//! business: `app_chrome::hooks::dom::range_rects` answers that for any subtree,
//! which is why a PDF's capture path uses it too.
//!
//! The arithmetic is pure and separate from the DOM walk so it is unit-testable
//! on the host: [`index_of_text_node`] (character offsets → a text node and an
//! offset inside it) and [`clamp_span`] (a span against the text that is
//! actually there). Finding the query in a row's text is not here at all — that
//! scan is `reader_core::search::occurrence_spans`, shared with both search
//! pipelines, so the ordinals a hit box counts and the ones a match carries
//! cannot drift apart.

use wasm_bindgen::JsCast;

/// Layers painted OVER a block's text, whose own text is not document text.
///
/// A gloss stroke's button carries the glossed word as its accessible name, and
/// a search hit's box is an empty sibling of the text it covers; counting either
/// would shift every offset after the first. The measure column renders every
/// block a second time and is skipped for the same reason — guarded rather than
/// excluded by construction, since it is mounted as a SIBLING of the page hosts
/// (`features/reader/page.rs`) and a walk that starts at a host never reaches it.
const OVERLAY_CLASSES: [&str; 3] = ["gloss-layer", "tx-hits", "tx-measure"];

/// The block's text nodes, in document order.
///
/// The walk is a plain `childNodes` recursion rather than a `TreeWalker`: it
/// needs no extra `web-sys` feature, and one block is a handful of nodes.
/// Nodes inside the block's own stroke layer are skipped — a mark's button
/// carries the glossed word as its accessible name, and counting that text
/// would shift every offset after the first mark.
fn text_nodes_of(el: &web_sys::Element) -> Vec<web_sys::Node> {
    let mut nodes = Vec::new();
    collect_text_nodes(el, &mut nodes);
    nodes
}

fn collect_text_nodes(node: &web_sys::Node, out: &mut Vec<web_sys::Node>) {
    match node.node_type() {
        web_sys::Node::TEXT_NODE => out.push(node.clone()),
        web_sys::Node::ELEMENT_NODE => {
            if let Some(el) = node.dyn_ref::<web_sys::Element>() {
                let classes = el.class_list();
                // An overlay's own text is not document text: a mark's button
                // carries the glossed word as its accessible name, and counting
                // either one would shift every offset after it.
                //
                // The measure column is the same case, guarded rather than
                // excluded by construction: it renders every block a second
                // time, but it is mounted as a SIBLING of the page hosts
                // (`features/reader/page.rs`), so this walk — which starts at a
                // host — never reaches it. The class check is what keeps the
                // offsets honest if it is ever moved inside one; it costs one
                // `DOMTokenList::contains` per element.
                if OVERLAY_CLASSES.iter().any(|name| classes.contains(name)) {
                    return;
                }
            }
            let children = node.child_nodes();
            for index in 0..children.length() {
                if let Some(child) = children.item(index) {
                    collect_text_nodes(&child, out);
                }
            }
        }
        _ => {}
    }
}

/// Convert an offset counted in CHARACTERS (what a `ReflowSpot` stores, and
/// what the engine's tracker reports) into the UTF-16 code-unit offset a DOM
/// `Range` wants, within one text node's content.
///
/// The two units agree for everything in the Basic Multilingual Plane and
/// differ only for supplementary characters — emoji, mathematical
/// alphanumerics — where one character is two code units. Converting at this
/// one boundary is what lets the stored identity be honest characters on both
/// sides of the wire while the DOM still gets what it asked for.
fn utf16_offset_for_char(content: &str, char_offset: usize) -> u32 {
    content
        .chars()
        .take(char_offset)
        .map(|ch| ch.len_utf16() as u32)
        .sum()
}

/// Which text node holds character `offset`, and how far into it that is.
///
/// Pure: `lengths` is a block's text nodes in order. An offset at or past the
/// end lands on the last node's end (or on `(0, 0)` for a block with no text),
/// so a mark whose document was edited shorter still projects onto something
/// sane instead of failing.
fn index_of_text_node(lengths: &[u32], offset: usize) -> (usize, u32) {
    let mut remaining = offset;
    for (index, &length) in lengths.iter().enumerate() {
        if remaining < length as usize {
            return (index, remaining as u32);
        }
        remaining -= length as usize;
    }
    match lengths.last() {
        Some(&last) => (lengths.len() - 1, last),
        None => (0, 0),
    }
}

/// A `[start, end)` span clamped into a block that now holds `chars`
/// characters. Ordered, so a clamped span can never come back backwards.
pub(crate) fn clamp_span(start: usize, end: usize, chars: usize) -> (usize, usize) {
    let start = start.min(chars);
    (start, end.clamp(start, chars))
}

/// A DOM `Range` over `[start, end)` of `el`'s text, clamped to what is
/// actually there. `None` when the block holds no text at all.
pub(crate) fn range_for_span(el: &web_sys::Element, start: usize, end: usize) -> Option<web_sys::Range> {
    let document = web_sys::window()?.document()?;
    let nodes = text_nodes_of(el);
    let texts: Vec<web_sys::Text> = nodes
        .iter()
        .filter_map(|node| node.dyn_ref::<web_sys::Text>())
        .cloned()
        .collect();
    // Character counts, because that is the unit a spot counts in; the DOM is
    // handed a code-unit offset only at the `set_start`/`set_end` boundary.
    let contents: Vec<String> = texts.iter().map(|text| text.data()).collect();
    let lengths: Vec<u32> = contents.iter().map(|c| c.chars().count() as u32).collect();
    let total: usize = lengths.iter().map(|&length| length as usize).sum();
    if total == 0 {
        return None;
    }
    let (start, end) = clamp_span(start, end, total);
    let (start_node, start_offset) = index_of_text_node(&lengths, start);
    let (end_node, end_offset) = index_of_text_node(&lengths, end);
    let range = document.create_range().ok()?;
    range
        .set_start(
            nodes.get(start_node)?,
            utf16_offset_for_char(contents.get(start_node)?, start_offset as usize),
        )
        .ok()?;
    range
        .set_end(
            nodes.get(end_node)?,
            utf16_offset_for_char(contents.get(end_node)?, end_offset as usize),
        )
        .ok()?;
    Some(range)
}

/// The rendered text of a block row, one entry per text node, in document order.
///
/// Concatenated, this is the row's own coordinate system: the offsets
/// [`match_spans`] reports and [`range_for_span`] accepts are counts into it.
fn text_contents(el: &web_sys::Element) -> Vec<String> {
    text_nodes_of(el)
        .iter()
        .filter_map(|node| node.dyn_ref::<web_sys::Text>())
        .map(|text| text.data())
        .collect()
}

/// Every occurrence of `needle` in a block row's rendered text, as character
/// spans in the row's own coordinate system.
///
/// The scan is the one both search pipelines run
/// (`reader_core::search::occurrence_spans`), which is what makes the nth span
/// here the nth hit the search found in this block — the pairing a highlight box
/// and an active match meet on. The row's text is folded here rather than kept,
/// because a row is walked on a query change and not on a keystroke of index
/// building.
pub(crate) fn match_spans(el: &web_sys::Element, needle: &str) -> Vec<(usize, usize)> {
    let text = text_contents(el).concat();
    reader_core::search::occurrence_spans(&text, &text.to_lowercase(), needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_land_inside_their_own_text_node() {
        // Three text nodes: 5, 0 and 7 characters.
        let lengths = [5u32, 0, 7];
        assert_eq!(index_of_text_node(&lengths, 0), (0, 0));
        assert_eq!(index_of_text_node(&lengths, 4), (0, 4));
        // A zero-length node holds no characters, so nothing lands inside it:
        // the offset that would have is the next node's start.
        assert_eq!(index_of_text_node(&lengths, 5), (2, 0));
        assert_eq!(index_of_text_node(&lengths, 6), (2, 1));
        assert_eq!(index_of_text_node(&lengths, 11), (2, 6));
        // Past the end: the last node's end, not a panic and not a wrap.
        assert_eq!(index_of_text_node(&lengths, 12), (2, 7));
        assert_eq!(index_of_text_node(&lengths, 999), (2, 7));
    }

    #[test]
    fn an_empty_block_has_nowhere_to_put_an_offset() {
        assert_eq!(index_of_text_node(&[], 0), (0, 0));
        assert_eq!(index_of_text_node(&[], 40), (0, 0));
    }

    #[test]
    fn a_single_text_node_counts_from_its_own_start() {
        let lengths = [11u32];
        assert_eq!(index_of_text_node(&lengths, 0), (0, 0));
        assert_eq!(index_of_text_node(&lengths, 7), (0, 7));
        assert_eq!(index_of_text_node(&lengths, 11), (0, 11));
        assert_eq!(index_of_text_node(&lengths, 12), (0, 11));
    }

    #[test]
    fn character_offsets_convert_to_the_code_units_a_dom_range_wants() {
        // Plain ASCII: the two units agree, so nothing moves.
        assert_eq!(utf16_offset_for_char("palimpsest", 0), 0);
        assert_eq!(utf16_offset_for_char("palimpsest", 4), 4);
        // A supplementary character is ONE character and TWO code units, so
        // every offset after it shifts by one — the whole reason the spot is
        // stored in characters and converted here, at the DOM's boundary.
        let with_emoji = "ab\u{1F600}cd";
        assert_eq!(utf16_offset_for_char(with_emoji, 2), 2);
        assert_eq!(utf16_offset_for_char(with_emoji, 3), 4);
        assert_eq!(utf16_offset_for_char(with_emoji, 5), 6);
        // Past the end is the node's whole length: a clamped spot still
        // resolves to a real offset rather than throwing at `set_end`.
        assert_eq!(utf16_offset_for_char(with_emoji, 99), 6);
        assert_eq!(utf16_offset_for_char("", 3), 0);
    }

    #[test]
    fn spans_clamp_into_the_text_that_is_there() {
        assert_eq!(clamp_span(2, 8, 20), (2, 8));
        assert_eq!(clamp_span(2, 80, 20), (2, 20));
        assert_eq!(clamp_span(30, 80, 20), (20, 20));
        // A backwards span collapses; it never inverts.
        assert_eq!(clamp_span(9, 3, 20), (9, 9));
        assert_eq!(clamp_span(0, 0, 0), (0, 0));
    }
}
