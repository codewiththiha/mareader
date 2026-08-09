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

pub fn custom_zoom_clamp(s: f64) -> f64 {
    clamp_scale(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_width_uses_container_width() {
        let s = fit_scale(FitMode::Width, 600.0, 800.0, 300.0, 400.0, 0.0, 1.0);
        assert!((s - 2.0).abs() < 1e-9);
    }

    #[test]
    fn fit_page_takes_min_of_width_and_height() {
        // 300x400 page in 500x1000 -> height-limited (500/300=1.66 vs 1000/400=2.5)
        let s = fit_scale(FitMode::Page, 500.0, 1000.0, 300.0, 400.0, 0.0, 1.0);
        assert!((s - 1.6666).abs() < 1e-3);
        // in 500x600 -> width-limited (1.6666 vs 1.5)
        let s2 = fit_scale(FitMode::Page, 500.0, 600.0, 300.0, 400.0, 0.0, 1.0);
        assert!((s2 - 1.5).abs() < 1e-9);
    }

    #[test]
    fn fit_respects_padding() {
        let s = fit_scale(FitMode::Width, 600.0, 800.0, 300.0, 400.0, 20.0, 1.0);
        assert!((s - (580.0 / 300.0)).abs() < 1e-9);
    }

    #[test]
    fn scale_clamped() {
        assert_eq!(clamp_scale(0.1), MIN_SCALE);
        assert_eq!(clamp_scale(99.0), MAX_SCALE);
        assert_eq!(clamp_scale(1.0), 1.0);
    }

    #[test]
    fn zoom_steps() {
        assert!((nearest_zoom(1.0, 1) - 1.25).abs() < 1e-9);
        assert!((nearest_zoom(1.0, -1) - 0.9).abs() < 1e-9);
        assert!((nearest_zoom(0.1, -1) - 0.25).abs() < 1e-9);
        assert!((nearest_zoom(99.0, 1) - 5.0).abs() < 1e-9);
        // non-preset current value steps to nearest adjacent preset
        assert!((nearest_zoom(1.2, 1) - 1.25).abs() < 1e-9);
    }
}
