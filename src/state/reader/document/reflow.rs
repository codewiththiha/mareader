//! The reflowable half of the open document: its blocks, and the page cut the
//! reader is currently reading them through.
//!
//! The PDF keeps its content in the engine; a plain-text or Markdown document
//! keeps it HERE — the whole file parsed into blocks the moment it opens
//! (`txt_core::parse_plain_text` / `md_core::parse_markdown`, both cutting
//! through `reflow_core::block`). The paginator (`reflow_core::pager`) packs
//! the blocks into A4 pages, published as three parallel signals the layouts
//! read:
//!
//! * `heights` — every block's best-known height at scale 1, seeded by the
//!   pure estimate at open, then refined as the reader's own rows report
//!   measured heights (`crate::effects::reader::reflow_measure`);
//! * `cuts` — the page split those heights produce;
//! * `block_page` — the inverse map (block → page) navigation and search
//!   resolve positions through.
//!
//! Heights and cuts move through two doors that always write the split and its
//! map together — [`ReflowContent::set_initial_heights`] at open,
//! [`ReflowContent::recut`] afterwards — because a split that disagrees with
//! its map would send a page jump to the wrong block.

use std::sync::Arc;

use leptos::prelude::*;

use md_core::MarkdownHeading;
use reflow_core::block::TextBlock;
use reflow_core::geometry::{PageGeometry, PAGE_HEIGHT};
use reflow_core::pager::{block_page_index, first_block_of_page, paginate, BlockMetrics, PageCut};
use reflow_core::typography::TextSettings;
use virtual_list_leptos::Virtualizer;

/// The blocks, their heights and the current page cut of one reflowable
/// document. The format, the title and the chapter tree live on
/// [`super::DocumentState`] with every other document's identity.
#[derive(Clone, Copy)]
pub struct ReflowContent {
    /// The parsed document, in block order. A shared handle rather than a
    /// `Vec` because Leptos clones a signal's value per reader: a novel is
    /// thousands of blocks and a page turn re-reads the list (pages, stream,
    /// highlight pass). Cloning the handle is a refcount bump; cloning the
    /// list was several thousand allocations per notify.
    pub blocks: RwSignal<Arc<Vec<TextBlock>>>,
    /// The document's headings, for a format that has them. Kept beside the
    /// blocks rather than as a finished outline because a Markdown heading has
    /// no page number until the cut says so — the outline is re-projected onto
    /// `block_page` whenever the cut moves
    /// (`crate::effects::reader::reflow_outline`).
    pub headings: RwSignal<Arc<Vec<MarkdownHeading>>>,
    /// Block heights at scale 1 — estimate-seeded, measurement-refined.
    pub heights: RwSignal<Arc<Vec<f64>>>,
    /// The current page split of those heights.
    pub cuts: RwSignal<Arc<Vec<PageCut>>>,
    /// Bumped every time the split is re-published, so a consumer that only
    /// needs to know THAT the pages moved watches one integer instead of
    /// comparing the whole cut — the gloss layer's invalidation fingerprint,
    /// re-derived on every scroll frame of a reflowable document.
    ///
    /// It counts re-cuts, not changes: [`ReflowContent::recut`] bumps it
    /// whenever it publishes, including the rare re-measure that lands on the
    /// split it already had. Over-notifying costs one pass of the consumers —
    /// what they would have paid to find nothing moved; under-notifying would
    /// strand a mark on the wrong page.
    pub cut_generation: RwSignal<u64>,
    /// Block → 0-based page under the current split.
    pub block_page: RwSignal<Arc<Vec<u32>>>,
    /// The geometry the current split was cut with (a book-layout toggle or a
    /// width dial re-cuts through the measurement pipeline).
    pub geometry: RwSignal<PageGeometry>,
    /// The continuous stream's virtualizer while that layout is mounted
    /// (`None` otherwise). The stream — not the page-cut strip — scrolls
    /// reflowable documents vertically, and the readers that aim it (search
    /// reveal, the bottom bar's scrubber) reach it through here rather than a
    /// second wiring through the page virtualizers, which in this mode are
    /// deliberately unbound.
    pub stream: StoredValue<Option<Virtualizer>, LocalStorage>,
    /// The fractional reading position the open flow found in the library
    /// (0..=1), for the stream to anchor on when it mounts. Consumed by the
    /// anchor itself, so a later remount anchors on the page instead.
    pub resume_fraction: RwSignal<Option<f64>>,
    /// The stream's current extent (the virtualizer's total size), mirrored by
    /// the stream layout as it changes. The virtualizer's own signals are
    /// thread-local — unreadable from the `Send` closures the chrome builds
    /// (progress strip, percentage indicator) — so the one number chrome needs
    /// travels through this plain signal.
    pub stream_total: RwSignal<f64>,
}

impl Default for ReflowContent {
    fn default() -> Self {
        Self {
            blocks: RwSignal::new(Arc::new(Vec::new())),
            headings: RwSignal::new(Arc::new(Vec::new())),
            heights: RwSignal::new(Arc::new(Vec::new())),
            cuts: RwSignal::new(Arc::new(Vec::new())),
            cut_generation: RwSignal::new(0),
            block_page: RwSignal::new(Arc::new(Vec::new())),
            geometry: RwSignal::new(PageGeometry::default()),
            stream: StoredValue::new_local(None),
            resume_fraction: RwSignal::new(None),
            stream_total: RwSignal::new(0.0),
        }
    }
}

impl ReflowContent {
    /// Back to the no-document state. Every field the open flow writes is
    /// reset here, so a field added to the struct cannot be silently
    /// forgotten by close.
    pub fn reset(&self) {
        self.blocks.set(Arc::new(Vec::new()));
        self.headings.set(Arc::new(Vec::new()));
        self.heights.set(Arc::new(Vec::new()));
        self.cuts.set(Arc::new(Vec::new()));
        self.cut_generation.set(0);
        self.block_page.set(Arc::new(Vec::new()));
        // The geometry too: it is the split's other half (what the heights were
        // cut against), and leaving the previous document's book-layout gutter
        // behind would let a stale `geometry` claim a re-cut is needed — or not
        // needed — for a document that has no blocks at all.
        self.geometry.set(PageGeometry::default());
        self.stream.set_value(None);
        self.resume_fraction.set(None);
        self.stream_total.set(0.0);
    }

    /// The live stream virtualizer, when the continuous text stream is
    /// mounted. (The handle lives in local storage — the virtualizer is not
    /// `Send` — so callers read it where they run, on the UI thread.)
    pub fn stream_handle(&self) -> Option<Virtualizer> {
        self.stream.try_with_value(|v| v.clone()).flatten()
    }

    /// How many blocks the open document has, read untracked. The pages, the
    /// stream and the search pass all need it; none should subscribe to the
    /// block list itself just to learn its length.
    pub fn block_count(&self) -> usize {
        self.blocks.with_untracked(|blocks| blocks.len())
    }

    /// One block by index, read untracked (the render paths read it tracked
    /// through their own `For`, which owns the key).
    pub fn block_at(&self, index: usize) -> Option<TextBlock> {
        self.blocks.with_untracked(|blocks| blocks.get(index).cloned())
    }

    /// The open document's identity as a `<For>` key: the block list's `Arc`
    /// pointer. Keying on it means a different document remounts its blocks
    /// instead of reusing index keys the outgoing file occupied — which is why
    /// this read is TRACKED: the key is only worth having if a new document
    /// can reach the closure that computes it.
    pub fn document_id(&self) -> usize {
        self.blocks.with(|blocks| Arc::as_ptr(blocks) as usize)
    }

    /// Open-time: install the given heights — the pure estimate — as the
    /// standing truth, and publish the page cut they produce.
    ///
    /// Every later refinement arrives through [`Self::recut`]: measurements
    /// write the heights, the re-estimate writes the heights, and the cut
    /// follows the best-known numbers. Splitting the two doors lets a
    /// measurement land without a full re-cut's paperwork when nothing moved,
    /// and a dial move re-cut without a measurement in sight.
    pub fn set_initial_heights(
        &self,
        state: crate::state::AppState,
        heights: Vec<f64>,
        geo: PageGeometry,
    ) -> super::ReflowCut {
        let cuts = paginate(&heights, geo.content_height);
        self.heights.set(Arc::new(heights));
        self.publish_cut(state, cuts, geo)
    }

    /// Re-cut the pages from the best-known heights at `geo`. Answers `None` —
    /// writing nothing — when neither the split nor the geometry moved since
    /// the last publish, so a jitter-sized measurement cannot wake the whole
    /// paginated side of the reader.
    ///
    /// The caller owns the heights write (a measurement batch or re-estimate
    /// lands first); this method owns the split, its inverse map and the
    /// generation, and answers what the document's shared page machinery must
    /// be told ([`super::ReflowCut`]). The page count and per-page sizes
    /// belong to the document, so they are RETURNED rather than poked into a
    /// sibling's signals: a document deciding its own pagination is a re-cut;
    /// a document setting the reader's page count is one module writing
    /// another's state. The caller hands the answer to
    /// [`super::DocumentState::publish_cut`].
    pub fn recut(
        &self,
        state: crate::state::AppState,
        geo: PageGeometry,
    ) -> Option<super::ReflowCut> {
        let heights = self.heights.get_untracked();
        let cuts = paginate(&heights, geo.content_height);
        let unchanged = self.cuts.with_untracked(|old| old.as_slice() == cuts.as_slice())
            && self.geometry.with_untracked(|old| *old == geo);
        if unchanged {
            return None;
        }
        Some(self.publish_cut(state, cuts, geo))
    }

    /// The shared tail of both doors: write the split, its inverse map and
    /// the generation, hold the reader on the block they were reading, and
    /// assemble the answer for the document's page machinery.
    fn publish_cut(
        &self,
        state: crate::state::AppState,
        cuts: Vec<PageCut>,
        geo: PageGeometry,
    ) -> super::ReflowCut {
        let map = block_page_index(&cuts, self.block_count());

        // Where the reader was, in BLOCKS — survives the re-cut.
        let prev_page = state.reader.viewer.page.get_untracked();
        let anchor_block = self
            .cuts
            .with_untracked(|old| first_block_of_page(old, prev_page));
        let new_page = map.get(anchor_block).map_or(1, |p| p + 1);

        let n = cuts.len() as u32;

        self.cuts.set(Arc::new(cuts));
        self.cut_generation.update(|generation| *generation += 1);
        self.block_page.set(Arc::new(map));
        self.geometry.set(geo);

        // The sheet is the cut's one fixed point: every page of a reflowable
        // document is the same size, so the sizes travel as one page and one
        // height, not two vectors of a repeated value. The WIDTH is the dialled
        // card (the column-width dial grows the sheet with the column); the
        // height is at the live display scale — the scale the cut was measured
        // at.
        super::ReflowCut {
            num_pages: n,
            page_size: pdf_engine::types::PageSize { width: geo.width, height: PAGE_HEIGHT },
            css_height: PAGE_HEIGHT * state.reader.viewer.zoom.visual_scale(),
            page: new_page.clamp(1, n.max(1)),
        }
    }

    /// The stream's reading position as 0..=1, or `None` while no stream is
    /// mounted. Purely a snapshot: persistence calls it inside its effect,
    /// where the tracked reads that matter (the page, the mode) already run.
    pub fn stream_fraction(&self) -> Option<f64> {
        let v = self.stream_handle()?;
        let total = v.total_size().get_untracked();
        let viewport = v.viewport().get_untracked().main;
        let offset = v.scroll_offset().get_untracked();
        let extent = (total - viewport).max(0.0);
        Some(if extent > 0.0 { (offset / extent).clamp(0.0, 1.0) } else { 0.0 })
    }
}

/// The estimate's block metrics for a typography + page geometry — the inputs
/// of `reflow_core::pager::estimate_block_height`, gathered once per
/// pagination run.
pub fn estimate_metrics(settings: &TextSettings, geo: &PageGeometry) -> BlockMetrics {
    BlockMetrics {
        content_width: geo.content_width,
        font_size: settings.font_size,
        line_height: settings.line_height,
        paragraph_margin_em: settings.paragraph_margin,
        char_width: reflow_core::typography::body_char_width(settings),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_estimate_metrics_carry_the_settings_through() {
        let mut s = TextSettings::default();
        s.font_size = 20.0;
        s.line_height = 1.5;
        s.paragraph_margin = 0.5;
        let geo = reflow_core::geometry(false);
        let m = estimate_metrics(&s, &geo);
        assert_eq!(m.font_size, 20.0);
        assert_eq!(m.line_height, 1.5);
        assert_eq!(m.paragraph_margin_em, 0.5);
        assert_eq!(m.content_width, geo.content_width);
        assert!(m.char_width > 0.0);
    }
}
