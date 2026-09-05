//! The page geometry every format publishes: how big each page is.
//!
//! This used to be the PDF half of the document, and the name said so
//! (`PdfContent`, reached as `content.pdf`). But nothing in it is PDF's. A page
//! size is known twice over — the intrinsic (scale-1) box the document declares,
//! and the CSS-px height the laid-out page actually took — and both drive things
//! that must not be asked about the format they are sizing: the strip
//! virtualizer seeds from them, the zoom coordinator anchors against them, the
//! blend backdrop reads the heights, and the thumbnails' row pitch is derived
//! from the first sheet.
//!
//! A PDF fills them from the file (`services::document::open::seed`, refined by
//! the engine's geometry callback as pages render). A reflowable document fills
//! them from its page cut, with A4 as the one fixed point, through
//! [`PageMetrics::publish_uniform`] — which is exactly why the field cannot keep
//! a format's name: the reader's paged modes, its zoom ladder and its progress
//! chrome all read this and none of them may care who counted.
//!
//! `page1_size` is the answer every fixed-geometry surface uses before a page
//! has rendered, which is why the fallback policy sits on the document rather
//! than in each surface.

use leptos::prelude::*;

use pdf_engine::types::PageSize;

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
}

impl PageMetrics {
    /// Back to "no pages". Called by [`super::DocumentState::reset`], and by
    /// nothing else: the two vectors must move together, or a strip lays out
    /// against heights from the book that was just closed.
    pub fn reset(&self) {
        self.page1_size.set(None);
        self.intrinsic.set(Vec::new());
        self.css_heights.set(Vec::new());
    }

    /// Publish a page count whose pages are all one size — a reflowable cut,
    /// where A4 is the one fixed point.
    ///
    /// Both vectors are written only when they would actually change.
    /// `intrinsic` is an input to the virtualizers' geometry epoch, so handing
    /// them a fresh (but identical) A4 column on every re-measure rebuilt both
    /// page layouts — the second, redundant rewindow a reader saw right after a
    /// text document settled onto its measured cut. A re-cut that keeps the page
    /// count has nothing to tell them, and a zoom never reaches this at all (the
    /// stream rescales itself, the paged modes go through
    /// `crate::effects::reader::reflow_layout`).
    ///
    /// The height tolerance is half a CSS pixel: these are laid-out heights at a
    /// fractional scale, and re-measuring the same cut must not read as a change
    /// because the scale rounded differently.
    pub fn publish_uniform(&self, count: u32, size: &PageSize, css_height: f64) {
        let pages = count as usize;
        let sizes_current = self
            .intrinsic
            .with_untracked(|sizes| sizes.len() == pages && sizes.iter().all(|page| page == size));
        if !sizes_current {
            self.intrinsic.set(vec![size.clone(); pages]);
        }
        let heights_current = self.css_heights.with_untracked(|store| {
            store.len() == pages && store.iter().all(|h| (h - css_height).abs() < 0.5)
        });
        if !heights_current {
            self.css_heights.set(vec![css_height; pages]);
        }
    }

    /// The vertical strip's size model: a page's laid-out CSS height, plus the
    /// gap after it.
    ///
    /// One definition because four moments read it and must agree — the no-gap
    /// pref, the page-margin pref, a reflowable re-cut and a zoom rescale. A
    /// strip that sized its pages one way and then re-sized them another on the
    /// next rescale would walk the reader's position by a gap per page, which is
    /// the kind of drift no single call site can see.
    ///
    /// The heights are read live per item rather than snapshotted into the
    /// closure: the store is what a rescale has just written, and copying a
    /// whole book's heights only to hand them straight back is the allocation
    /// the zoom path was written to avoid. The horizontal strip has its own
    /// model (intrinsic widths times scale, plus margin on the scroll axis) and
    /// deliberately does not share this one.
    pub fn strip_sizes(&self, gap: f64) -> impl Fn(usize) -> f64 {
        let heights = self.css_heights;
        move |index: usize| {
            heights.with_untracked(|store| store.get(index).copied().unwrap_or(0.0)) + gap
        }
    }
}
