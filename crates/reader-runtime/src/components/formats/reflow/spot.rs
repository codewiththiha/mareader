//! Where a reflowable document's characters are, in the DOM.

use wasm_bindgen::JsCast;

/// Layers painted OVER a block's text, whose text is not document
/// text.
const OVERLAY_CLASSES: [&str; 2] = ["gloss-layer", "tx-hits"];

/// The block's text nodes, in document order.
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
                // An overlay's text is not document
                // text; counting it would shift offsets.
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

/// A character offset → the UTF-16 code-unit offset a `Range` wants,
/// within one node.
fn utf16_offset_for_char(content: &str, char_offset: usize) -> u32 {
    content
        .chars()
        .take(char_offset)
        .map(|ch| ch.len_utf16() as u32)
        .sum()
}

/// Which text node holds character `offset`, and how far into it.
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

/// A `[start, end)` span clamped into a block of `chars` characters.
pub(crate) fn clamp_span(start: usize, end: usize, chars: usize) -> (usize, usize) {
    let start = start.min(chars);
    (start, end.clamp(start, chars))
}

/// A `Range` over `[start, end)`, clamped to what is there.
pub(crate) fn range_for_span(
    el: &web_sys::Element,
    start: usize,
    end: usize,
) -> Option<web_sys::Range> {
    let document = web_sys::window()?.document()?;
    let nodes = text_nodes_of(el);
    let texts: Vec<web_sys::Text> = nodes
        .iter()
        .filter_map(|node| node.dyn_ref::<web_sys::Text>())
        .cloned()
        .collect();
    // Character counts, because that is a spot's unit.
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

/// The row's rendered text, one entry per text node.
fn text_contents(el: &web_sys::Element) -> Vec<String> {
    text_nodes_of(el)
        .iter()
        .filter_map(|node| node.dyn_ref::<web_sys::Text>())
        .map(|text| text.data())
        .collect()
}

/// Every occurrence of `needle`, as spans in the row's own text.
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
        // A zero-length node holds nothing; the offset is the next node's.
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
        // A supplementary character is ONE character and TWO code units.
        let with_emoji = "ab\u{1F600}cd";
        assert_eq!(utf16_offset_for_char(with_emoji, 2), 2);
        assert_eq!(utf16_offset_for_char(with_emoji, 3), 4);
        assert_eq!(utf16_offset_for_char(with_emoji, 5), 6);
        // Past the end is the node's whole length, not a throw.
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
