//! The outline's jump into the continuous stream.

use leptos::prelude::*;

use md_core::MarkdownHeading;
use virtual_list_leptos::Align;

use crate::state::ReaderState;

use super::navigation_sync::scroll_mode;

/// The block an outline entry opens, `None` for a stale index.
fn block_of_entry(headings: &[MarkdownHeading], index: usize) -> Option<usize> {
    headings.get(index).map(|heading| heading.block_index)
}

/// Install the outline → stream arm (one per pane, at its mount).
pub fn outline_jump(state: ReaderState) {
    // One derived signal, so runs do not pile up.
    let zooming = state.viewer.zooming();
    Effect::new(move |_| {
        // A zoom transaction re-anchors the strip; the directive waits.
        if zooming.get() {
            return;
        }
        let Some(index) = state.viewer.take_outline_jump() else {
            return;
        };
        let reflow = state.document.content.reflow;
        let Some(stream) = reflow.stream_handle() else {
            return;
        };
        let block = reflow
            .headings
            .with_untracked(|h| block_of_entry(h, index as usize));
        let Some(block) = block else {
            return;
        };
        stream.scroll_to_index(block, Align::Start, scroll_mode(state));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heading(title: &str, block_index: usize) -> MarkdownHeading {
        MarkdownHeading {
            title: title.into(),
            level: 2,
            block_index,
        }
    }

    /// An entry resolves to its heading's block, or misses.
    #[test]
    fn an_entry_addresses_the_block_its_heading_opens() {
        let headings = [heading("One", 0), heading("Two", 4), heading("Three", 9)];
        assert_eq!(block_of_entry(&headings, 0), Some(0));
        assert_eq!(block_of_entry(&headings, 2), Some(9));
        assert_eq!(block_of_entry(&headings, 3), None);
        assert_eq!(block_of_entry(&[], 0), None);
    }
}
