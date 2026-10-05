//! Chrome-facing signal projection from the document's reports. Keeping
//! this separate makes iframe handoff and teardown read as lifetime code.

use leptos::prelude::*;

use super::{Inner, Mirror, put};

impl Inner {
    /// The live frame's report, written into the mirror. `reported` is set
    /// first, so the write-forwarding effects see their own value.
    pub(super) fn apply_mirror(&self, m: Mirror) {
        *self.reported.borrow_mut() = Some(m.clone());
        let reader = self.ctx.reader;
        let document = &reader.document;
        put(document.status, m.status);
        put(document.format, m.format);
        put(document.error, m.error);
        put(document.path, m.path);
        put(document.book_id, m.book_id);
        put(document.title, m.title);
        put(document.author, m.author);
        put(document.num_pages, m.num_pages);
        put(document.outline_pending, m.outline_pending);
        put(document.content.metrics.page1_size, m.page1);
        put(reader.viewer.page, m.page);
        put(reader.viewer.mode, m.mode);
        put(reader.viewer.fit, m.fit);
        put(reader.viewer.zoom.display, m.zoom);
        put(reader.viewer.auto_scroll, m.auto_scroll);
        put(reader.search.visible, m.search_visible);
        put(reader.viewer.first_paint, m.first_paint);
        let launch_changed = self.ctx.launch.try_with_untracked(|l| *l != m.launch) == Some(true);
        if launch_changed {
            // Another document: the rail's pictures are another book's.
            self.thumbs.reset();
            self.ctx.launch.set(m.launch);
        }
    }

    pub(super) fn apply_outline(&self, entries: &crate::pane_wire::WireOutline) {
        let nodes: Vec<reader_core::outline::OutlineNode> = entries
            .iter()
            .map(|(title, page, depth)| {
                reader_core::outline::OutlineNode::new(title.clone(), *page, *depth)
            })
            .collect();
        let outline = self.ctx.reader.document.outline;
        if outline.try_with_untracked(|o| o.as_slice() != nodes.as_slice()) == Some(true) {
            outline.set(std::sync::Arc::new(nodes));
        }
    }
}
