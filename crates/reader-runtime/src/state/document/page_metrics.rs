//! The page geometry every format publishes: how big each page is.
//!
//! This used to be the PDF half of the document, and the name said so
//! (`PdfContent`). But nothing in it is PDF's: a page size is known twice over
//! — the intrinsic (scale-1) box the document declares, and the CSS-px height
//! the laid-out page took — and both drive things that must not be asked about
//! the format they size: the strip virtualizer seeds from them, the zoom
//! coordinator anchors against them, the blend backdrop reads the heights, and
//! the thumbnails' row pitch derives from the first sheet.
//!
//! A PDF fills them from the file (`services::document::open::seed`, refined
//! by the engine's geometry callback as pages render). A reflowable document
//! fills them from its page cut, A4 the one fixed point, through
//! [`PageMetrics::publish_uniform`] — exactly why the field cannot keep a
//! format's name.
//!
//! `page1_size` is the answer every fixed-geometry surface uses before a page
//! has rendered, which is why the fallback policy sits on the document rather
//! than in each surface.

use leptos::prelude::*;

use pdf_engine::types::PageSize;

/// How close two laid-out heights must be to count as the same one: half a
/// CSS pixel. These are heights at a fractional scale, so re-measuring the
/// same cut must not read as a change because the scale rounded differently.
const HEIGHT_EPSILON: f64 = 0.5;

/// How far a rendered page's implied scale-1 size may sit from the size the
/// fit maths already holds before it counts as a different page size, in
/// scale-1 CSS px. Covers the engine's whole-pixel rounding down to the 25%
/// zoom floor.
const RENDERED_SIZE_TOLERANCE: f64 = 4.0;

/// The open document's page sizes, at scale 1 and as laid out.
#[derive(Clone, Copy, Default)]
pub struct PageMetrics {
    /// CSS-px size of page 1 at scale 1 (used for fit modes before any render).
    pub page1_size: RwSignal<Option<PageSize>>,
    /// Intrinsic (scale-1) width/height of every page, 0-based.
    pub intrinsic: RwSignal<Vec<PageSize>>,
    /// Rendered CSS-px heights per page, seeded from `intrinsic` and refined
    /// by `on_geometry` as pages actually render.
    pub css_heights: RwSignal<Vec<f64>>,
    /// Scale-1 sizes the engine actually rasterised, 0-based, `None` until a
    /// page has rendered. The open seeds `intrinsic` with page 1's box for
    /// every page (a serial size probe over a long book looked like a hang),
    /// so this is the only place a page's TRUE size is known. It feeds the
    /// fit modes only: never the virtualizers, so recording a render cannot
    /// rebuild a layout. Written untracked for the same reason.
    pub rendered: RwSignal<Vec<Option<PageSize>>>,
}

impl PageMetrics {
    /// Record the scale-1 size page `page` (1-based) rendered at. Returns
    /// `true` when it differs from what the fit maths believed before. Safe
    /// after teardown: a render completion can outlive the reader state.
    pub fn record_rendered(&self, page: u32, width: f64, height: f64) -> bool {
        if page == 0 || !(width > 0.0 && height > 0.0) {
            return false;
        }
        let index = (page - 1) as usize;
        let Some(before) = self.fit_size(page) else {
            return false;
        };
        let size = PageSize { width, height };
        let stored = self.rendered.try_update_untracked(|store| {
            if store.len() <= index {
                store.resize(index + 1, None);
            }
            store[index] = Some(size);
        });
        // The engine reports whole CSS px at the render scale, so the scale-1
        // size it implies carries up to `1 / scale` px of rounding. Only a
        // real difference counts — a rounding wobble must never chain into a
        // refit, re-render, refit loop.
        let differs = |a: f64, b: f64| (a - b).abs() > RENDERED_SIZE_TOLERANCE.max(b * 0.005);
        stored.is_some() && (differs(before.0, width) || differs(before.1, height))
    }

    /// The scale-1 size a fit should measure page `page` (1-based) by: the
    /// rendered size when the page has rendered, else its declared box, else
    /// page 1's. `None` before any document is seeded (or after teardown).
    pub fn fit_size(&self, page: u32) -> Option<(f64, f64)> {
        let index = page.max(1) as usize - 1;
        let usable =
            |s: &PageSize| (s.width > 0.0 && s.height > 0.0).then_some((s.width, s.height));
        if let Some(Some(hit)) = self
            .rendered
            .try_with_untracked(|store| store.get(index).cloned().flatten())
            .map(|s| s.as_ref().and_then(usable))
        {
            return Some(hit);
        }
        if let Some(Some(hit)) = self
            .intrinsic
            .try_with_untracked(|sizes| sizes.get(index).and_then(usable))
        {
            return Some(hit);
        }
        self.page1_size
            .try_get_untracked()
            .flatten()
            .and_then(|s| usable(&s))
    }

    /// Forget every rendered size: a new document is being seeded.
    pub fn clear_rendered(&self) {
        if self.rendered.with_untracked(|store| !store.is_empty()) {
            self.rendered.update_untracked(Vec::clear);
        }
    }

    /// Whether the laid-out heights already are `sizes`, within
    /// [`HEIGHT_EPSILON`]. The write guard every writer of `css_heights` owes
    /// the virtualizers' geometry epoch: a write that changes nothing still
    /// bumps the epoch, and the epoch rebuilds both page layouts.
    pub fn heights_agree(&self, sizes: &[f64]) -> bool {
        self.css_heights.with_untracked(|store| {
            store.len() == sizes.len()
                && store
                    .iter()
                    .zip(sizes)
                    .all(|(a, b)| (a - b).abs() < HEIGHT_EPSILON)
        })
    }

    /// Publish a page count whose pages are all one size — a reflowable cut,
    /// where A4 is the one fixed point.
    ///
    /// Both vectors are written only when they would actually change:
    /// `intrinsic` is an input to the virtualizers' geometry epoch, so handing
    /// them a fresh-but-identical A4 column on every re-measure rebuilt both
    /// page layouts — the redundant rewindow a reader saw right after a text
    /// document settled onto its measured cut. A re-cut that keeps the count
    /// has nothing to tell them, and a zoom never reaches this at all (the
    /// stream rescales itself; the paged modes go through
    /// `reader_runtime::effects::reader::reflow_layout`).
    pub fn publish_uniform(&self, count: u32, size: &PageSize, css_height: f64) {
        self.clear_rendered();
        let pages = count as usize;
        let sizes_current = self
            .intrinsic
            .with_untracked(|sizes| sizes.len() == pages && sizes.iter().all(|page| page == size));
        if !sizes_current {
            self.intrinsic.set(vec![size.clone(); pages]);
        }
        let heights = vec![css_height; pages];
        if !self.heights_agree(&heights) {
            self.css_heights.set(heights);
        }
    }

    /// The vertical strip's size model: a page's laid-out CSS height, plus the
    /// gap after it.
    ///
    /// One definition because four moments read it and must agree — the no-gap
    /// pref, the page-margin pref, a reflowable re-cut and a zoom rescale. A
    /// strip that sized its pages one way and re-sized them another on the
    /// next rescale would walk the reader's position by a gap per page — drift
    /// no single call site can see.
    ///
    /// Heights are read live per item rather than snapshotted into the
    /// closure: the store is what a rescale just wrote, and copying a whole
    /// book's heights only to hand them straight back is the allocation the
    /// zoom path was written to avoid. The horizontal strip has its own model
    /// (intrinsic widths times scale, plus margin on the scroll axis).
    pub fn strip_sizes(&self, gap: f64) -> impl Fn(usize) -> f64 {
        let heights = self.css_heights;
        move |index: usize| {
            heights.with_untracked(|store| store.get(index).copied().unwrap_or(0.0)) + gap
        }
    }
}
