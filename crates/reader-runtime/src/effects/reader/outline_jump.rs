//! The outline's jump into the continuous stream: a clicked chapter scrolls
//! the text to the block its heading opens.
//!
//! The page write the same click makes cannot do this. A text document's page
//! is a CUT of the stream — several chapters share one, a short document has
//! one, and no reader ever sees a page boundary — so a page number is both too
//! coarse (the stream would land on the page's first block, usually a
//! different chapter) and, for a one-cut document, blunt (every chapter on
//! page 1 would jump to the top). The heading's BLOCK is the exact address,
//! and it never leaves the pane: a Markdown outline entry IS a heading, since
//! `md_core::headings_to_nodes` maps the heading list one-to-one and in order,
//! so an entry's index resolves to `headings[index].block_index` — documented
//! as stable for the whole session, because a block is never re-cut once it
//! exists.
//!
//! Why a directive instead of the click doing it: the panel that clicks and
//! the stream that scrolls share one realm only when the pane runs in-process.
//! A pane realm renders its chrome host-side, so the click writes the pane
//! state the two sides share, the host hands the directive to the frame
//! (`Write::Outline`), and this arm consumes it either way.
//!
//! The jump lands on the stream's own handle. The page-cut virtualizer is
//! deliberately unbound in this mode (see `navigation_sync::dominant`), and a
//! stream that is not mounted is the same ordinary miss a search reveal has:
//! the click's page write still landed, so the reader is never left
//! mid-nothing. A zoom transaction in flight is the one thing a jump waits
//! for — the arm's own comment says why.

use leptos::prelude::*;

use md_core::MarkdownHeading;
use virtual_list_leptos::Align;

use crate::state::ReaderState;

use super::navigation_sync::scroll_mode;

/// The block the outline entry at `index` opens — the stream's address for a
/// chapter. `None` for an index no heading answers: a stale entry (another
/// document's outline, a rail outliving its document) is not a jump.
fn block_of_entry(headings: &[MarkdownHeading], index: usize) -> Option<usize> {
    headings.get(index).map(|heading| heading.block_index)
}

/// Install the outline → stream arm (one per pane, at its mount).
pub fn outline_jump(state: ReaderState) {
    // One derived signal, made once: the arm must be woken when a transaction
    // closes, and a signal derived per run would pile up in the pane's owner.
    let zooming = state.viewer.zooming();
    Effect::new(move |_| {
        // A zoom transaction re-scales the strip and re-anchors it, and the
        // jump would fight that anchor. The directive WAITS instead of being
        // lost: this tracked read is what re-runs the arm on the frame the
        // transaction closes, and the pending index is read only once it has.
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
        let block = reflow.headings.with_untracked(|h| block_of_entry(h, index as usize));
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

    /// An entry resolves to the block its own heading opens, and an index no
    /// heading answers is a miss rather than a jump to nowhere.
    #[test]
    fn an_entry_addresses_the_block_its_heading_opens() {
        let headings = [heading("One", 0), heading("Two", 4), heading("Three", 9)];
        assert_eq!(block_of_entry(&headings, 0), Some(0));
        assert_eq!(block_of_entry(&headings, 2), Some(9));
        assert_eq!(block_of_entry(&headings, 3), None);
        assert_eq!(block_of_entry(&[], 0), None);
    }
}
