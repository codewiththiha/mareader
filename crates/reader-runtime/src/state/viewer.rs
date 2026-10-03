//! The viewer signals: which page, which mode, how big the container is —
//! and the live copy of the shared [`Motion`] projection that says which of
//! the reader's movements are allowed to animate (the type itself is chrome
//! state, `app_state::state`, because the shell and both runtimes hand it
//! around; the SIGNAL is this session's).

use leptos::prelude::*;

use app_state::state::Motion;
use reader_core::view::{PAGE_GAP, ViewMode};
use reader_core::zoom_math::FitMode;

use super::zoom::ZoomState;

/// The zoom pipeline signals (see `reader_runtime::effects`).
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
    /// The engine's selectionchange listener walks the DOM from the
    /// selection's anchor and focus up to the nearest page host, parses the
    /// page from its id, and dispatches `mareader:selection-pages`;
    /// `effects::reader::page_selection` is the single writer of this signal,
    /// and `features::virtualizers` merges the range into the
    /// virtualizer's PINNED window so the selected pages stay mounted while
    /// the selection lives.
    pub selected_pages: RwSignal<Option<(u32, u32)>>,
    /// Continuous auto-scroll along the active strip (Continuous / Horizontal).
    pub auto_scroll: RwSignal<bool>,
    /// Inter-page gap in the continuous strip (0 when No Gap is on).
    pub page_gap: RwSignal<f64>,
    /// Horizontal inset around pages (CSS px). `0` removes the margin.
    pub page_margin: RwSignal<f64>,
    /// The column-width dial, as the open reader resolves it (percent:
    /// `100` is the natural column). Mirrored from the persisted setting by
    /// the layout prefs so the surfaces that have no settings handle — the
    /// stream's column, the fit maths — read one runtime number, the same
    /// arrangement the page margin uses.
    pub column_width_pct: RwSignal<f64>,
    /// Which motions animate. Written only by the runtime session's
    /// projection of the frame's settings (`Motion::from_prefs`, in the
    /// runtime's `lib.rs`); see the type's contract.
    pub motion: RwSignal<Motion>,
    /// The pane's own appearance look while independent themes are on
    /// (`None` = inherit the window's theme). Written only by the host's
    /// appearance boundary push ([`crate::host::contract::PaneAppearance`]);
    /// the pane root paints it in `crate::pane::view`.
    pub look: RwSignal<Option<reader_core::appearance::Appearance>>,
    /// True from the moment `page` is seeded for a freshly opened document
    /// until a scrolling strip has anchored itself to that page on mount.
    ///
    /// The resume point is authored by the open flow, not by the strip, so
    /// until the strip has been placed on it the strip's own dominant page
    /// (still whatever offset it last held, usually the top) is not an
    /// opinion worth listening to. The scroll→page sync stands down while
    /// this is raised; the strip's mount anchor lowers it.
    pub awaiting_anchor: RwSignal<bool>,
    /// Monotonic identity of the scrolling strip anchor that currently owns
    /// `awaiting_anchor`. A replacement strip can start before the old
    /// strip's queued animation frame runs; the identity keeps that stale
    /// callback from releasing the replacement's guard.
    pub(crate) anchor_generation: RwSignal<u64>,
    /// The one-shot gate over this open's first VISIBLE frame — false in the
    /// fresh state a document's realm builds with, until the page the reader
    /// should see has actually PAINTED. The release is paint-driven, and each
    /// surface owns its own: every PDF mode lifts it on a successful
    /// current-page raster, even when its geometry is unchanged. Text
    /// lifts it when its mount anchor lands (or, without an anchor, its
    /// first mounted frame paints). Only text has a timed anchor net; PDF
    /// startup deadlines report errors instead of pretending it painted. For
    /// exactly that long an opaque cover the colour of the reader's paper
    /// masks the viewer there, so the first renders — however healthy — are
    /// never watched arriving: the reader appears already settled on the
    /// resume page.
    pub first_paint: RwSignal<bool>,
}

impl ViewerSignals {
    /// Claim `awaiting_anchor` for a new scrolling-strip anchor.
    pub fn begin_anchor(&self) -> u64 {
        let generation = self.anchor_generation.get_untracked().wrapping_add(1);
        self.anchor_generation.set(generation);
        generation
    }

    pub fn owns_anchor(&self, generation: u64) -> bool {
        self.anchor_generation.get_untracked() == generation
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

    /// [`Self::zooming_now`] for callbacks that can outlive the reader's own
    /// owner — a queued frame, a geometry report racing a close. `None` is
    /// "this state is gone": there is no transition to be in, and the caller
    /// returns instead of reading a disposed signal.
    pub fn try_zooming_now(&self) -> Option<bool> {
        self.zoom
            .transition
            .try_get_untracked()
            .map(|t| t.is_some())
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
            fit.get_untracked() == FitMode::None && transition.get().is_some_and(|t| !t.following)
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
            column_width_pct: RwSignal::new(100.0),
            motion: RwSignal::new(Motion::default()),
            look: RwSignal::new(None),
            awaiting_anchor: RwSignal::new(false),
            anchor_generation: RwSignal::new(0),
            first_paint: RwSignal::new(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reader_core::settings::AnimationSettings;

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
    fn each_detail_owns_exactly_the_motion_it_names() {
        // `scroll_glide` is spelled `scroll_jumps` in the settings, so this
        // projection is the only place the two vocabularies meet — a crossed
        // wire there moves the wrong motion, which is why every line is
        // exercised rather than the one that happens to share a name.
        macro_rules! drops_exactly {
            ($pref:ident -> $motion:ident) => {{
                let mut prefs = AnimationSettings::default();
                prefs.$pref = false;
                let m = Motion::from_prefs(&prefs);
                assert!(!m.$motion, "{} must drop its own motion", stringify!($pref));
                let dropped = [m.sidebar_slide, m.canvas_resize, m.zoom, m.scroll_glide]
                    .iter()
                    .filter(|on| !**on)
                    .count();
                assert_eq!(dropped, 1, "{} must drop nothing else", stringify!($pref));
            }};
        }
        drops_exactly!(sidebar_slide -> sidebar_slide);
        drops_exactly!(canvas_resize -> canvas_resize);
        drops_exactly!(zoom -> zoom);
        drops_exactly!(scroll_jumps -> scroll_glide);
    }
}
