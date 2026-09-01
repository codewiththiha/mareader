//! The viewer signals: which page, which mode, how big the container is —
//! and the motion projection that says which of the reader's movements are
//! allowed to animate.

use leptos::prelude::*;

use pdf_core::layout::{PAGE_GAP, ViewMode};
use pdf_core::math::FitMode;
use pdf_core::settings::AnimationSettings;

use super::zoom::ZoomState;

/// Which of the reader's motions animate. Projected from the persisted
/// [`AnimationSettings`] by the app root (`effects::app::motion::publish_motion`)
/// and read by everything that moves a page, so no consumer has to know that a
/// master switch exists: the projection already applied it.
///
/// Read TRACKED by views (the rail's transition class has to change when the
/// reader flips a switch) and UNTRACKED by effects and scroll calls: a flag
/// that stops something animating must not be what triggers the animation.
///
/// Nothing here skips a change. Off renders the end frame in the frame the
/// change arrives, which is why freezing the reader loses no fit, no follow
/// and no scroll target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Motion {
    /// The rail animates its open/close: the docked rail tweens its width
    /// (`SIDEBAR_SLIDE_MS`), the floating rail fades (`SIDEBAR_FADE_MS`).
    pub sidebar_slide: bool,
    /// The page rides a window drag: the canvas flexes on every frame of it.
    /// Riding the RAIL is not in here on purpose — a container that was
    /// measured is answered in the same frame, animation or no animation, and
    /// deferring it is what cropped the page for a visible instant.
    pub canvas_resize: bool,
    /// A zoom eases to its target over the profile's duration.
    pub zoom: bool,
    /// A jump to a page glides the column (or the thumbnail rail) over it.
    pub scroll_glide: bool,
}

impl Motion {
    /// The one place the master switch is honoured. Off, no detail can bring
    /// an animation back on; the Animations tab hides itself for the same
    /// reason, so the detail switches are never shown lying.
    pub const fn from_prefs(p: &AnimationSettings) -> Self {
        Self {
            sidebar_slide: p.enabled && p.sidebar_slide,
            canvas_resize: p.enabled && p.canvas_resize,
            zoom: p.enabled && p.zoom,
            scroll_glide: p.enabled && p.scroll_jumps,
        }
    }
}

impl Default for Motion {
    /// Everything moves. The shell publishes the reader's prefs before
    /// anything can act on them, and a reader that has not been published to
    /// yet (a document opening, a test) must not look broken.
    fn default() -> Self {
        Self {
            sidebar_slide: true,
            canvas_resize: true,
            zoom: true,
            scroll_glide: true,
        }
    }
}

/// The zoom pipeline signals (see `crate::effects`).
#[derive(Clone, Copy)]
pub struct ViewerSignals {
    pub mode: RwSignal<ViewMode>,
    /// 1-based current page.
    pub page: RwSignal<u32>,
    pub fit: RwSignal<FitMode>,
    pub scroll_top: RwSignal<f64>,
    pub zoom: ZoomState,
    /// (width, height) of the viewer content area in CSS px.
    pub container_size: RwSignal<(f64, f64)>,
    /// Inclusive `(first, last)` 1-based page range of the reader's current
    /// text selection, or `None` when no text is selected.
    ///
    /// The `pdfEngine.ts` selectionchange listener walks the DOM from the
    /// selection's anchor and focus up to the nearest `.pdf-page` host, parses
    /// the page index from its id (`cont-{i}-pg`), and dispatches a
    /// `pdfreader:selection-pages` CustomEvent with `{ first, last }` (or
    /// `null` to clear). This effect listens for that event and writes the
    /// range here so `PageList` can PIN those pages in the virtualization
    /// window — otherwise scrolling evicts them, orphaning the selection's
    /// DOM nodes and breaking copy of multi-page selections.
    pub selected_pages: RwSignal<Option<(u32, u32)>>,
    /// Continuous auto-scroll along the active strip (Continuous / Horizontal).
    pub auto_scroll: RwSignal<bool>,
    /// Inter-page gap in the continuous strip (0 when No Gap is on).
    pub page_gap: RwSignal<f64>,
    /// Horizontal inset around pages (CSS px). `0` removes the margin.
    pub page_margin: RwSignal<f64>,
    /// Which motions animate. Written only by the shell, from the settings
    /// (`Motion::from_prefs`); see the type's contract.
    pub motion: RwSignal<Motion>,
}

impl ViewerSignals {
    /// Reset the reading position (page + scroll) on document close. Kept
    /// separate from a full reset: fit/zoom state is the reader's, not the
    /// document's.
    pub fn reset_position(&self) {
        self.page.set(1);
        self.scroll_top.set(0.0);
        self.auto_scroll.set(false);
        self.page_gap.set(PAGE_GAP);
        self.page_margin.set(0.0);
    }

    /// True while a zoom transaction is in flight: renders are suspended,
    /// page/scroll synchronisation and geometry feedback are frozen, and the
    /// mounted window is pinned around the dominant page.
    pub fn zooming(&self) -> Signal<bool> {
        let transition = self.zoom.transition;
        Signal::derive(move || transition.get().is_some())
    }

    /// Untracked variant of [`Self::zooming`] for rAF/scroll callbacks and
    /// effect guards that must not subscribe to the transition.
    pub fn zooming_now(&self) -> bool {
        self.zoom.transition.get_untracked().is_some()
    }

    /// True only while a manual zoom animation is in flight (fit is `None`,
    /// so the reader is zooming by hand rather than re-fitting). When set,
    /// the layouts hand the canvas to the gesture so a fit-driven refit can
    /// never fight the pinch.
    ///
    /// A container follow is deliberately excluded even though it opens a
    /// transition too: a window drag with a hand-picked zoom is not a gesture,
    /// and pages must not start rasterising at a display scale that is already
    /// obsolete two frames later.
    pub fn gesture_owns(&self) -> Signal<bool> {
        let transition = self.zoom.transition;
        let fit = self.fit;
        Signal::derive(move || {
            fit.get_untracked() == FitMode::None
                && transition.get().is_some_and(|t| !t.following)
        })
    }
}

impl Default for ViewerSignals {
    fn default() -> Self {
        Self {
            mode: RwSignal::new(ViewMode::ScrollVertical),
            page: RwSignal::new(1),
            fit: RwSignal::new(FitMode::None),
            scroll_top: RwSignal::new(0.0),
            zoom: ZoomState::default(),
            container_size: RwSignal::new((800.0, 600.0)),
            selected_pages: RwSignal::new(None),
            auto_scroll: RwSignal::new(false),
            page_gap: RwSignal::new(PAGE_GAP),
            page_margin: RwSignal::new(0.0),
            motion: RwSignal::new(Motion::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_master_switch_freezes_every_detail() {
        let all_on = AnimationSettings::default();
        assert!(all_on.enabled);
        let m = Motion::from_prefs(&all_on);
        assert!(m.sidebar_slide && m.canvas_resize && m.zoom && m.scroll_glide);

        // With the master off, no detail can bring an animation back.
        let frozen = AnimationSettings {
            enabled: false,
            ..AnimationSettings::default()
        };
        assert!(frozen.zoom && frozen.sidebar_slide);
        let m = Motion::from_prefs(&frozen);
        assert!(!m.sidebar_slide);
        assert!(!m.canvas_resize);
        assert!(!m.zoom);
        assert!(!m.scroll_glide);
    }

    #[test]
    fn a_detail_switch_moves_exactly_one_motion() {
        // The tab offers one switch per motion, so each has to own precisely
        // the one it names — and the master has to stay out of their way.
        let p = AnimationSettings {
            zoom: false,
            ..AnimationSettings::default()
        };
        let m = Motion::from_prefs(&p);
        assert!(!m.zoom);
        assert!(m.sidebar_slide && m.canvas_resize);
        assert!(m.scroll_glide);
    }
}
