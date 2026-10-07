//! The viewer signals: page, mode, container size, and the live motion
//! projection.

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
    /// The outline entry just asked for, as an index; ONE-SHOT.
    pub(crate) outline_jump: RwSignal<Option<u32>>,
    /// The zoom step just asked for: `1` in, `-1` out; ONE-SHOT.
    pub(crate) zoom_step: RwSignal<Option<i32>>,
    pub fit: RwSignal<FitMode>,
    pub scroll_top: RwSignal<f64>,
    pub zoom: ZoomState,
    /// (width, height) of the viewer content area in CSS px.
    pub container_size: RwSignal<(f64, f64)>,
    /// Inclusive `(first, last)` page range of the current selection, or
    /// `None`.
    pub selected_pages: RwSignal<Option<(u32, u32)>>,
    /// Continuous auto-scroll along the active strip (Continuous / Horizontal).
    pub auto_scroll: RwSignal<bool>,
    /// Inter-page gap in the continuous strip (0 when No Gap is on).
    pub page_gap: RwSignal<f64>,
    /// Horizontal inset around pages (CSS px). `0` removes the margin.
    pub page_margin: RwSignal<f64>,
    /// The column-width dial, in percent, as the open reader resolves it.
    pub column_width_pct: RwSignal<f64>,
    /// Which motions animate; written only by the session projection.
    pub motion: RwSignal<Motion>,
    /// The pane's own look while independent themes are on.
    pub look: RwSignal<Option<reader_core::appearance::Appearance>>,
    /// True from the resume seed until a strip has anchored to the page.
    pub awaiting_anchor: RwSignal<bool>,
    /// Identity of the strip anchor that owns `awaiting_anchor`.
    pub(crate) anchor_generation: RwSignal<u64>,
    /// The one-shot gate over this open's first VISIBLE frame.
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

    /// Ask for the outline entry at `index`, the panel's row click.
    pub fn ask_outline_jump(&self, index: u32) {
        self.outline_jump.set(Some(index));
    }

    /// Take the pending outline jump, clearing it; the read is TRACKED.
    pub(crate) fn take_outline_jump(&self) -> Option<u32> {
        let index = self.outline_jump.get()?;
        self.outline_jump.set(None);
        Some(index)
    }

    /// Ask for one zoom step: `1` in, `-1` out (the toolbar's buttons).
    pub fn ask_zoom_step(&self, step: i32) {
        self.zoom_step.set(Some(step));
    }

    /// Take the pending zoom step, clearing it; the read is TRACKED.
    pub(crate) fn take_zoom_step(&self) -> Option<i32> {
        let step = self.zoom_step.get()?;
        self.zoom_step.set(None);
        Some(step)
    }

    /// True while a zoom transaction is in flight: renders are suspended.
    pub fn zooming(&self) -> Signal<bool> {
        let transition = self.zoom.transition;
        Signal::derive(move || transition.get().is_some())
    }

    /// [`Self::zooming`] untracked, for callbacks that must not subscribe.
    pub fn zooming_now(&self) -> bool {
        self.zoom.transition.get_untracked().is_some()
    }

    /// [`Self::zooming_now`] for callbacks that can outlive the owner.
    pub fn try_zooming_now(&self) -> Option<bool> {
        self.zoom
            .transition
            .try_get_untracked()
            .map(|t| t.is_some())
    }

    /// True only while a manual zoom animation is in flight.
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
            outline_jump: RwSignal::new(None),
            zoom_step: RwSignal::new(None),
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
        // `scroll_glide` is `scroll_jumps` in the settings; this projection
        // joins them.
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
