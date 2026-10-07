//! The page geometry every format publishes: how big each page is.

use leptos::prelude::*;

use reader_core::document::PageSize;

/// How close two laid-out heights must be to count as the same.
const HEIGHT_EPSILON: f64 = 0.5;

/// How far a rendered page's implied scale-1 size may sit.
const RENDERED_SIZE_TOLERANCE: f64 = 4.0;

/// The open document's page sizes, at scale 1 and as laid out.
#[derive(Clone, Copy, Default)]
pub struct PageMetrics {
    /// CSS-px size of page 1 at scale 1 (used for fit modes before any render).
    pub page1_size: RwSignal<Option<PageSize>>,
    /// Intrinsic (scale-1) width/height of every page, 0-based.
    pub intrinsic: RwSignal<Vec<PageSize>>,
    /// Laid-out heights per page, seeded from `intrinsic`.
    pub css_heights: RwSignal<Vec<f64>>,
    /// Scale-1 sizes the engine actually rasterised; `None` until
    /// rendered.
    pub rendered: RwSignal<Vec<Option<PageSize>>>,
}

impl PageMetrics {
    /// Record the scale-1 size page `page` rendered at.
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
        // Whole CSS px at the render scale: only a real difference counts.
        let differs = |a: f64, b: f64| (a - b).abs() > RENDERED_SIZE_TOLERANCE.max(b * 0.005);
        stored.is_some() && (differs(before.0, width) || differs(before.1, height))
    }

    /// The size a fit should measure page `page` by.
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

    /// Whether the laid-out heights already are `sizes`.
    pub fn heights_agree(&self, sizes: &[f64]) -> bool {
        self.css_heights.with_untracked(|store| {
            store.len() == sizes.len()
                && store
                    .iter()
                    .zip(sizes)
                    .all(|(a, b)| (a - b).abs() < HEIGHT_EPSILON)
        })
    }

    /// Publish a uniform page count, where A4 is the one fixed point.
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

    /// The vertical strip's size model: height plus the gap after it.
    pub fn strip_sizes(&self, gap: f64) -> impl Fn(usize) -> f64 {
        let heights = self.css_heights;
        move |index: usize| {
            heights.with_untracked(|store| store.get(index).copied().unwrap_or(0.0)) + gap
        }
    }
}
