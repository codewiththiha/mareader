//! Pure zoom/fit math. No wasm deps — unit-testable on the host.

pub const MIN_SCALE: f64 = 0.25;
pub const MAX_SCALE: f64 = 5.0;

/// Zoom presets shown in the zoom menu / used by +/- stepping.
pub const ZOOM_STEPS: &[f64] = &[
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    None,
    Width,
    Page,
}

pub fn clamp_scale(s: f64) -> f64 {
    s.clamp(MIN_SCALE, MAX_SCALE)
}

/// Intrinsic (scale-1) size of page `page` (1-based) from the parallel
/// width/height columns the engine returns on open.
///
/// Fit and shrink-to-fit must use the page under the reader's eyes, not
/// page 1. A landscape plate in an otherwise-portrait book is cropped if
/// the ceiling is computed from the letter pages around it; a portrait
/// page after a plate would stay over-shrunk the other way.
///
/// Missing / non-positive entries fall back to `fallback_w` / `fallback_h`
/// (typically page 1), so an older engine that only reported heights still
/// produces a usable size.
pub fn page_intrinsic(
    page: u32,
    widths: &[f64],
    heights: &[f64],
    fallback_w: f64,
    fallback_h: f64,
) -> (f64, f64) {
    let i = page.saturating_sub(1) as usize;
    let w = widths
        .get(i)
        .copied()
        .filter(|w| *w > 0.0)
        .unwrap_or(fallback_w);
    let h = heights
        .get(i)
        .copied()
        .filter(|h| *h > 0.0)
        .unwrap_or(fallback_h);
    (w.max(1.0), h.max(1.0))
}


/// Scale that fits a page (base CSS-px size `page_w` x `page_h`) into a
/// container of `container_w` x `container_h`, leaving `padding` px of air.
/// `FitMode::None` should never call this — it returns the caller's current scale.
pub fn fit_scale(
    fit: FitMode,
    container_w: f64,
    container_h: f64,
    page_w: f64,
    page_h: f64,
    padding: f64,
    current: f64,
) -> f64 {
    match fit {
        FitMode::None => clamp_scale(current),
        FitMode::Width => {
            let w = (container_w - padding).max(1.0);
            clamp_scale(w / page_w.max(1.0))
        }
        FitMode::Page => {
            let w = (container_w - padding).max(1.0);
            let h = (container_h - padding).max(1.0);
            clamp_scale((w / page_w.max(1.0)).min(h / page_h.max(1.0)))
        }
    }
}

/// The scale a manually-zoomed page should be shown at, given the space
/// actually available: a hand-zoomed page shrinks to fit when the window
/// narrows (never silently crops), and grows back to exactly what the reader
/// chose when the space returns. `desired` is the reader's choice (a zoom
/// gesture only); the result is `min(desired, fit_w)`, recomputed from
/// `desired` each time so shrinking is lossless and never drifts.
pub fn constrained_scale(desired: f64, fit_w: f64) -> f64 {
    if !desired.is_finite() || !fit_w.is_finite() || fit_w <= 0.0 {
        return clamp_scale(desired);
    }
    clamp_scale(desired.min(fit_w))
}

/// Whether `available` is enough to show `desired` without cropping.
///
/// Used to decide whether the page is currently being held below the reader's
/// chosen zoom, which the zoom readout needs in order to explain itself.
pub fn is_space_constrained(desired: f64, fit_w: f64) -> bool {
    desired.is_finite() && fit_w.is_finite() && fit_w > 0.0 && desired > fit_w + 1e-9
}

/// Next/previous zoom preset. `dir > 0` zooms in, `dir < 0` zooms out.
pub fn nearest_zoom(current: f64, dir: i32) -> f64 {
    if dir > 0 {
        ZOOM_STEPS
            .iter()
            .copied()
            .find(|&z| z > current + 1e-9)
            .unwrap_or(*ZOOM_STEPS.last().unwrap())
    } else if dir < 0 {
        ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|&z| z < current - 1e-9)
            .unwrap_or(ZOOM_STEPS[0])
    } else {
        clamp_scale(current)
    }
}

#[cfg(test)]
mod tests {
    use super::{constrained_scale, is_space_constrained};

    #[test]
    fn a_page_that_fits_is_left_completely_alone() {
        // The common case: plenty of room, so the reader's zoom is honoured.
        assert_eq!(constrained_scale(1.0, 2.0), 1.0);
        assert_eq!(constrained_scale(0.5, 2.0), 0.5);
        // Exactly fitting is still fitting.
        assert_eq!(constrained_scale(1.5, 1.5), 1.5);
    }

    #[test]
    fn a_page_too_big_for_the_space_shrinks_to_fit() {
        // THE BUG THIS FIXES: this used to stay at 2.0 and get cropped.
        assert_eq!(constrained_scale(2.0, 0.8), 0.8);
    }

    #[test]
    fn shrinking_never_pushes_a_zoomed_out_reader_back_up() {
        // Someone at 50% in a huge window must stay at 50%; this constraint
        // only ever removes scale, it is not a fit mode.
        assert_eq!(constrained_scale(0.5, 3.0), 0.5);
    }

    #[test]
    fn the_original_zoom_is_recovered_exactly_when_the_space_returns() {
        // The whole point of keeping `desired` separate: no drift, no matter
        // how many intermediate widths the window passed through.
        let desired = 2.5;
        let mut seen = Vec::new();
        for fit_w in [2.0, 1.2, 0.6, 0.9, 1.8, 4.0] {
            seen.push(constrained_scale(desired, fit_w));
        }
        assert_eq!(seen, vec![2.0, 1.2, 0.6, 0.9, 1.8, 2.5]);
        // Back to exactly what the reader chose, not 2.4999 or 2.5001.
        assert_eq!(*seen.last().unwrap(), desired);
    }

    #[test]
    fn the_result_never_exceeds_what_the_reader_asked_for() {
        // Growing back must STOP at `desired`, however much room appears.
        for fit_w in [1.0, 5.0, 50.0] {
            assert!(constrained_scale(1.25, fit_w) <= 1.25);
        }
    }

    #[test]
    fn the_clamp_still_applies_at_the_extremes() {
        assert_eq!(constrained_scale(999.0, 999.0), super::MAX_SCALE);
        assert_eq!(constrained_scale(0.0001, 0.0001), super::MIN_SCALE);
    }

    #[test]
    fn a_useless_container_measurement_is_ignored_rather_than_collapsing_the_page() {
        // A zero/NaN width arrives during mount and while the sidebar animates.
        // Treating it as "fits nothing" would slam the page to MIN_SCALE.
        assert_eq!(constrained_scale(1.0, 0.0), 1.0);
        assert_eq!(constrained_scale(1.0, -3.0), 1.0);
        assert_eq!(constrained_scale(1.0, f64::NAN), 1.0);
        assert_eq!(constrained_scale(1.0, f64::INFINITY), 1.0);
    }

    #[test]
    fn constrained_is_reported_only_when_the_page_is_actually_being_held_back() {
        assert!(is_space_constrained(2.0, 1.0));
        assert!(!is_space_constrained(1.0, 2.0));
        // Equal is not constrained — it fits.
        assert!(!is_space_constrained(1.5, 1.5));
        // Garbage geometry is never reported as constrained.
        assert!(!is_space_constrained(2.0, 0.0));
        assert!(!is_space_constrained(2.0, f64::NAN));
    }

    use super::*;

    #[test]
    fn fit_uses_the_page_under_the_eyes_not_page_one() {
        // Portrait letter, then a landscape plate twice as wide.
        let widths = [612.0, 1224.0, 612.0];
        let heights = [792.0, 792.0, 792.0];
        assert_eq!(page_intrinsic(1, &widths, &heights, 1.0, 1.0), (612.0, 792.0));
        assert_eq!(page_intrinsic(2, &widths, &heights, 1.0, 1.0), (1224.0, 792.0));
        // Same container: the plate must fit at half the scale of the letter.
        let letter = fit_scale(FitMode::Width, 600.0, 800.0, 612.0, 792.0, 0.0, 1.0);
        let plate = fit_scale(FitMode::Width, 600.0, 800.0, 1224.0, 792.0, 0.0, 1.0);
        assert!((letter - 2.0 * plate).abs() < 1e-9, "letter {letter} plate {plate}");
        // A missing column falls back rather than inventing a zero-width page.
        assert_eq!(page_intrinsic(9, &[], &[], 612.0, 792.0), (612.0, 792.0));
    }

    /// Fit modes: width uses the container width, page takes the smaller of the
    /// two ratios (so the whole page shows), and padding comes off first.
    #[test]
    fn fit_modes() {
        let s = fit_scale(FitMode::Width, 600.0, 800.0, 300.0, 400.0, 0.0, 1.0);
        assert!((s - 2.0).abs() < 1e-9);
        // 300x400 page in 500x1000 -> height-limited (500/300=1.66 vs 1000/400=2.5).
        let s = fit_scale(FitMode::Page, 500.0, 1000.0, 300.0, 400.0, 0.0, 1.0);
        assert!((s - 1.6666).abs() < 1e-3);
        // ...and in 500x600 -> width-limited (1.6666 vs 1.5).
        let s = fit_scale(FitMode::Page, 500.0, 600.0, 300.0, 400.0, 0.0, 1.0);
        assert!((s - 1.5).abs() < 1e-9);
        // Padding is removed from the container before dividing.
        let s = fit_scale(FitMode::Width, 600.0, 800.0, 300.0, 400.0, 20.0, 1.0);
        assert!((s - (580.0 / 300.0)).abs() < 1e-9);
    }

    /// Zoom presets: clamped to the supported range, and stepping moves to the
    /// adjacent preset — including from a value that is not itself a preset.
    #[test]
    fn zoom_clamping_and_steps() {
        assert_eq!(clamp_scale(0.1), MIN_SCALE);
        assert_eq!(clamp_scale(99.0), MAX_SCALE);
        assert_eq!(clamp_scale(1.0), 1.0);
        for (from, dir, want) in [
            (1.0, 1, 1.25),
            (1.0, -1, 0.9),
            (0.1, -1, 0.25),
            (99.0, 1, 5.0),
            // A non-preset current value steps to the nearest adjacent preset.
            (1.2, 1, 1.25),
        ] {
            assert!((nearest_zoom(from, dir) - want).abs() < 1e-9, "{from} dir {dir}");
        }
    }
}

/// Hermite smoothstep: 0 below `edge0`, 1 above `edge1`, smooth between.
///
/// Pure easing math, not gloss-specific — it lived in `gloss` only because
/// that was its first consumer (the card content's opacity/interactivity
/// fade as the morph progresses). Any UI fade that must start and end with
/// zero derivative belongs here.
pub fn smoothstep(t: f64, edge0: f64, edge1: f64) -> f64 {
    let x = ((t - edge0) / (edge1 - edge0).max(0.0001)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

#[cfg(test)]
mod smoothstep_tests {
    use super::smoothstep;

    #[test]
    fn is_clamped_at_both_edges_and_smooth_between() {
        assert!((smoothstep(-1.0, 0.0, 1.0)).abs() < 1e-9);
        assert!((smoothstep(0.0, 0.0, 1.0)).abs() < 1e-9);
        assert!((smoothstep(1.0, 0.0, 1.0) - 1.0).abs() < 1e-9);
        assert!((smoothstep(2.0, 0.0, 1.0) - 1.0).abs() < 1e-9);
        // The midpoint of a smoothstep is exactly 0.5.
        assert!((smoothstep(0.5, 0.0, 1.0) - 0.5).abs() < 1e-9);
        // Monotonic across the ramp.
        let mut last = -1.0;
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            let s = smoothstep(t, 0.0, 1.0);
            assert!(s >= last, "non-monotonic at {t}: {s} < {last}");
            last = s;
        }
    }

    #[test]
    fn a_degenerate_or_inverted_edge_pair_still_terminates() {
        // edge1 == edge0: the tiny epsilon denominator must not blow up or
        // produce NaN; the result is clamped to one end.
        assert!(smoothstep(5.0, 5.0, 5.0).is_finite());
    }
}
