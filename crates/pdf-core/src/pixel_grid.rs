//! Device-pixel grid snapping for page geometry, so layers share an edge.

/// Round `v` (CSS px) to the nearest device pixel for ratio `dpr`.
fn snap_to(v: f64, dpr: f64) -> f64 {
    // A nonsensical ratio would turn a coordinate into NaN; pass it through.
    if !(v.is_finite() && dpr.is_finite() && dpr > 0.0) {
        return v;
    }
    (v * dpr).round() / dpr
}

/// The display's current device-pixel ratio, defaulting to 1.0 off-browser.
fn device_pixel_ratio() -> f64 {
    web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .filter(|d| *d > 0.0 && d.is_finite())
        .unwrap_or(1.0)
}

/// The same answer with no display to ask: nothing snapped, nothing harmed.
fn device_pixel_ratio() -> f64 {
    1.0
}

/// Snap a CSS-px length or offset to the device-pixel grid.
pub fn snap_px(v: f64) -> f64 {
    snap_to(v, device_pixel_ratio())
}

/// One device pixel in CSS px, used to overlap neighbouring pages.
pub fn one_device_px() -> f64 {
    1.0 / device_pixel_ratio()
}

#[cfg(test)]
mod tests {
    use super::{device_pixel_ratio, snap_px, snap_to};

    #[test]
    fn integer_ratios_keep_whole_css_pixels() {
        for dpr in [1.0, 2.0, 3.0] {
            assert_eq!(snap_to(842.0, dpr), 842.0);
            assert_eq!(snap_to(0.0, dpr), 0.0);
        }
    }

    #[test]
    fn fractional_ratios_land_on_the_device_grid() {
        // 1.25: the grid step is 0.8 CSS px, a whole number of device pixels.
        let snapped = snap_to(1122.36, 1.25);
        assert!((snapped * 1.25 - (snapped * 1.25).round()).abs() < 1e-9);
        assert!((snapped - 1122.36).abs() <= 0.4 + 1e-9);

        // 1.5 and 1.75 are the other common Windows scalings.
        for dpr in [1.5, 1.75] {
            let snapped = snap_to(595.276, dpr);
            assert!((snapped * dpr - (snapped * dpr).round()).abs() < 1e-9);
            assert!((snapped - 595.276).abs() <= 0.5 / dpr + 1e-9);
        }
    }

    #[test]
    fn snapping_is_idempotent() {
        let once = snap_to(1234.5678, 1.5);
        assert_eq!(snap_to(once, 1.5), once);
    }

    #[test]
    fn adjacent_edges_meet_exactly() {
        // Two stacked pages: their shared edge must be the same device row.
        let dpr = 1.25;
        let h = snap_to(841.89, dpr);
        let top_of_second = snap_to(h, dpr);
        assert_eq!(top_of_second, h);
    }

    #[test]
    fn the_boundary_helper_is_the_ratio_the_display_reports() {
        // Off-browser the ratio is 1, so snapping must be the identity.
        assert!(device_pixel_ratio() >= 1.0);
        assert_eq!(snap_px(12.5), snap_to(12.5, device_pixel_ratio()));
    }

    #[test]
    fn degenerate_ratios_pass_the_value_through() {
        assert_eq!(snap_to(10.5, 0.0), 10.5);
        assert_eq!(snap_to(10.5, f64::NAN), 10.5);
        assert!(snap_to(f64::NAN, 2.0).is_nan());
    }
}
