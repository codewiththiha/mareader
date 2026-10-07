//! The reflowable half of the document: its blocks and its page cut.

use std::sync::Arc;

use leptos::prelude::*;

use md_core::MarkdownHeading;
use reflow_core::block::TextBlock;
use reflow_core::geometry::{PAGE_HEIGHT, PageGeometry};
use reflow_core::pager::{BlockMetrics, PageCut, block_page_index, first_block_of_page, paginate};
use reflow_core::typography::TextSettings;
use virtual_list_leptos::Virtualizer;

/// The blocks, heights and page cut of one reflowable document.
#[derive(Clone, Copy)]
pub struct ReflowContent {
    /// The parsed document; a shared handle because Leptos clones per
    /// reader.
    pub blocks: RwSignal<Arc<Vec<TextBlock>>>,
    /// The document's headings: no page number until the cut says so.
    pub headings: RwSignal<Arc<Vec<MarkdownHeading>>>,
    /// Block heights at scale 1 — estimate-seeded, measurement-refined.
    pub heights: RwSignal<Arc<Vec<f64>>>,
    /// Wholesale writes to `heights`: the open seed and every re-estimate.
    pub estimate_generation: RwSignal<u64>,
    /// The current page split of those heights.
    pub cuts: RwSignal<Arc<Vec<PageCut>>>,
    /// Bumped on every re-publish of the split, for consumers that need
    /// only "the pages moved".
    pub cut_generation: RwSignal<u64>,
    /// Block → 0-based page under the current split.
    pub block_page: RwSignal<Arc<Vec<u32>>>,
    /// The geometry the current split was cut with.
    pub geometry: RwSignal<PageGeometry>,
    /// The stream's virtualizer while that layout is mounted; `None`
    /// otherwise.
    pub stream: StoredValue<Option<Virtualizer>, LocalStorage>,
    /// The library's fractional position to anchor the stream on.
    pub resume_fraction: RwSignal<Option<f64>>,
    /// The stream's extent, mirrored for `Send` chrome closures.
    pub stream_total: RwSignal<f64>,
}

impl Default for ReflowContent {
    fn default() -> Self {
        Self {
            blocks: RwSignal::new(Arc::new(Vec::new())),
            headings: RwSignal::new(Arc::new(Vec::new())),
            heights: RwSignal::new(Arc::new(Vec::new())),
            estimate_generation: RwSignal::new(0),
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
    /// Back to the no-document state; the handles are `Copy`, so no
    /// arena node leaks.
    pub fn reset(&self) {
        // The geometry goes with the split: it is what the heights were cut
        // against.
        let Self {
            blocks,
            headings,
            heights,
            estimate_generation,
            cuts,
            cut_generation,
            block_page,
            geometry,
            stream,
            resume_fraction,
            stream_total,
        } = *self;
        blocks.set(Arc::new(Vec::new()));
        headings.set(Arc::new(Vec::new()));
        heights.set(Arc::new(Vec::new()));
        estimate_generation.set(0);
        cuts.set(Arc::new(Vec::new()));
        cut_generation.set(0);
        block_page.set(Arc::new(Vec::new()));
        geometry.set(PageGeometry::default());
        stream.set_value(None);
        resume_fraction.set(None);
        stream_total.set(0.0);
    }

    /// The live stream virtualizer, when the stream layout is mounted.
    pub fn stream_handle(&self) -> Option<Virtualizer> {
        self.stream.try_with_value(|v| v.clone()).flatten()
    }

    /// How many blocks the open document has, read untracked.
    pub fn block_count(&self) -> usize {
        self.blocks.with_untracked(|blocks| blocks.len())
    }

    /// One block by index, read untracked (renders read it tracked).
    pub fn block_at(&self, index: usize) -> Option<TextBlock> {
        self.blocks
            .with_untracked(|blocks| blocks.get(index).cloned())
    }

    /// The document's identity as a `<For>` key: the block list's `Arc`
    /// pointer.
    pub fn document_id(&self) -> usize {
        self.blocks.with(|blocks| Arc::as_ptr(blocks) as usize)
    }

    /// Open-time: install the estimate as the standing heights and
    /// publish their cut.
    pub fn set_initial_heights(
        &self,
        state: crate::state::ReaderState,
        heights: Vec<f64>,
        geo: PageGeometry,
    ) -> super::ReflowCut {
        let cuts = paginate(&heights, geo.content_height);
        self.heights.set(Arc::new(heights));
        // A wholesale write: consumers tracking the geometry rebuild for it.
        self.estimate_generation
            .update(|generation| *generation += 1);
        self.publish_cut(state, cuts, geo)
    }

    /// Re-cut from the best-known heights; `None` when nothing moved. The
    /// caller owns the heights write.
    pub fn recut(
        &self,
        state: crate::state::ReaderState,
        geo: PageGeometry,
    ) -> Option<super::ReflowCut> {
        let heights = self.heights.get_untracked();
        let cuts = paginate(&heights, geo.content_height);
        let unchanged = self
            .cuts
            .with_untracked(|old| old.as_slice() == cuts.as_slice())
            && self.geometry.with_untracked(|old| *old == geo);
        if unchanged {
            return None;
        }
        Some(self.publish_cut(state, cuts, geo))
    }

    /// The shared tail of both doors: the split, the map, the reader's
    /// block.
    fn publish_cut(
        &self,
        state: crate::state::ReaderState,
        cuts: Vec<PageCut>,
        geo: PageGeometry,
    ) -> super::ReflowCut {
        let map = block_page_index(&cuts, self.block_count());

        // Where the reader was, in BLOCKS — survives the re-cut.
        let prev_page = state.viewer.page.get_untracked();
        let anchor_block = self
            .cuts
            .with_untracked(|old| first_block_of_page(old, prev_page));
        let new_page = map.get(anchor_block).map_or(1, |p| p + 1);

        let n = cuts.len() as u32;

        self.cuts.set(Arc::new(cuts));
        self.cut_generation.update(|generation| *generation += 1);
        self.block_page.set(Arc::new(map));
        self.geometry.set(geo);

        // The sheet is the cut's fixed point: one page size, at the live
        // display scale.
        super::ReflowCut {
            num_pages: n,
            page_size: reader_core::document::PageSize {
                width: geo.width,
                height: PAGE_HEIGHT,
            },
            css_height: PAGE_HEIGHT * state.viewer.zoom.visual_scale(),
            page: new_page.clamp(1, n.max(1)),
        }
    }

    /// The stream's reading position as 0..=1, or `None` with no stream.
    pub fn stream_fraction(&self) -> Option<f64> {
        let v = self.stream_handle()?;
        let total = v.total_size().get_untracked();
        let viewport = v.viewport().get_untracked().main;
        let offset = v.scroll_offset().get_untracked();
        Some(reader_core::view::scroll_fraction(offset, total, viewport))
    }
}

/// The estimate's block metrics for a typography and page geometry.
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
        let s = TextSettings {
            font_size: 20.0,
            line_height: 1.5,
            paragraph_margin: 0.5,
            ..Default::default()
        };
        let geo = reflow_core::geometry(false);
        let m = estimate_metrics(&s, &geo);
        assert_eq!(m.font_size, 20.0);
        assert_eq!(m.line_height, 1.5);
        assert_eq!(m.paragraph_margin_em, 0.5);
        assert_eq!(m.content_width, geo.content_width);
        assert!(m.char_width > 0.0);
    }
}
