//! Pure zoom/fit math. No wasm deps — unit-testable on the host.

pub const MIN_SCALE: f64 = 0.25;
pub const MAX_SCALE: f64 = 5.0;

/// Zoom presets shown in the zoom menu / used by +/- stepping.
pub const ZOOM_STEPS: &[f64] = &[
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    None,
    Width,
    Page,
}

pub fn clamp_scale(s: f64) -> f64 {
    s.clamp(MIN_SCALE, MAX_SCALE)
}

/// Scale that fits a page (base CSS-px size `page_w` x `page_h`) into a
/// container of `container_w` x `container_h`, edge to edge.
/// `FitMode::None` should never call this — it returns the caller's current scale.
pub fn fit_scale(
    fit: FitMode,
    container_w: f64,
    container_h: f64,
    page_w: f64,
    page_h: f64,
    current: f64,
) -> f64 {
    let w = container_w.max(1.0);
    let h = container_h.max(1.0);
    match fit {
        FitMode::None => clamp_scale(current),
        FitMode::Width => clamp_scale(w / page_w.max(1.0)),
        FitMode::Page => clamp_scale((w / page_w.max(1.0)).min(h / page_h.max(1.0))),
    }
}

/// One step along the preset ladder. `dir > 0` zooms in, `dir < 0` zooms out.
///
/// The step is taken from the preset CLOSEST to `current`, then clamped into
/// the ladder — so a reader already at the top or bottom stays put instead of
/// wrapping to the other end or falling off the array. A non-preset scale
/// (a fit width of 137%, say) therefore rounds onto the ladder first and
/// steps from there, which is what makes repeated presses feel even.
pub fn nearest_zoom(current: f64, dir: i32) -> f64 {
    if dir == 0 {
        return clamp_scale(current);
    }
    let mut closest_idx = 0usize;
    let mut min_diff = f64::MAX;
    for (i, &step) in ZOOM_STEPS.iter().enumerate() {
        let diff = (step - current).abs();
        if diff < min_diff {
            min_diff = diff;
            closest_idx = i;
        }
    }
    let last = (ZOOM_STEPS.len() - 1) as i32;
    let target_idx = (closest_idx as i32 + dir).clamp(0, last) as usize;
    ZOOM_STEPS[target_idx]
}

#[cfg(test)]
mod tests {
    use super::{clamp_scale, fit_scale, nearest_zoom, FitMode, MAX_SCALE, MIN_SCALE};

    #[test]
    fn fit_uses_the_page_under_the_eyes_not_page_one() {
        // Portrait letter, then a landscape plate twice as wide: the same
        // container must fit the plate at half the letter's scale.
        let letter = fit_scale(FitMode::Width, 600.0, 800.0, 612.0, 792.0, 1.0);
        let plate = fit_scale(FitMode::Width, 600.0, 800.0, 1224.0, 792.0, 1.0);
        assert!((letter - 2.0 * plate).abs() < 1e-9, "letter {letter} plate {plate}");
    }

    /// Fit modes: width uses the container width, page takes the smaller of the
    /// two ratios (so the whole page shows).
    #[test]
    fn fit_modes() {
        let s = fit_scale(FitMode::Width, 600.0, 800.0, 300.0, 400.0, 1.0);
        assert!((s - 2.0).abs() < 1e-9);
        // 300x400 page in 500x1000 -> height-limited (500/300=1.66 vs 1000/400=2.5).
        let s = fit_scale(FitMode::Page, 500.0, 1000.0, 300.0, 400.0, 1.0);
        assert!((s - 1.6666).abs() < 1e-3);
        // ...and in 500x600 -> width-limited (1.6666 vs 1.5).
        let s = fit_scale(FitMode::Page, 500.0, 600.0, 300.0, 400.0, 1.0);
        assert!((s - 1.5).abs() < 1e-9);
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
            // A non-preset current value rounds onto the ladder first, then
            // steps: 1.2 is closest to 1.25, so zooming in lands on 1.5.
            (1.2, 1, 1.5),
            (1.2, -1, 1.0),
        ] {
            assert!((nearest_zoom(from, dir) - want).abs() < 1e-9, "{from} dir {dir}");
        }
    }

    /// Pressing zoom-in at the maximum (or zoom-out at the minimum) must be
    /// a no-op, not a wrap to the other end of the ladder.
    #[test]
    fn stepping_at_the_ends_of_the_ladder_stays_put() {
        assert_eq!(nearest_zoom(MAX_SCALE, 1), MAX_SCALE);
        assert_eq!(nearest_zoom(MIN_SCALE, -1), MIN_SCALE);
        // Repeated presses at the ceiling cannot walk off the array either.
        assert_eq!(nearest_zoom(nearest_zoom(MAX_SCALE, 1), 1), MAX_SCALE);
        assert_eq!(nearest_zoom(nearest_zoom(MIN_SCALE, -1), -1), MIN_SCALE);
        // And a garbage scale lands somewhere sane rather than poisoning the
        // ladder for every later step.
        assert!(nearest_zoom(f64::NAN, 1).is_finite());
        assert_eq!(nearest_zoom(1.0, 0), 1.0);
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
