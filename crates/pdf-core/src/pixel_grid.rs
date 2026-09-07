//! Device-pixel grid snapping for page geometry — the PDF pipeline's
//! presentation boundary, living beside the canvas it protects. Both page
//! strips (raster and reflowable) snap through it, so the joint between two
//! sheets is the same arithmetic whichever painted them.
//!
//! Every page is a stack of independently rasterized compositor layers (the
//! canvas with its blend/filter, the texture `::before`, the backdrop), and
//! the compositor snaps each layer's paint rect to the DEVICE pixel grid.
//! Page geometry, though, is `intrinsic_size × zoom_scale` — almost never a
//! whole number of device pixels once `devicePixelRatio` is fractional (125% /
//! 150% / 175% scaling, any browser zoom). When two adjacent rects round in
//! opposite directions the result is a one-device-pixel GAP (dark backdrop as
//! a hairline) or OVERLAP (two blended paper surfaces composing on one row as
//! a near-black line) — the thin line at page joints and page sides that comes
//! and goes with zoom level.
//!
//! The cure is to write no fractional geometry in the first place: every
//! value that becomes a page's CSS size or position goes through [`snap_px`]
//! at the boundary, so neighbouring layers resolve to the same device-pixel
//! edge. Internal maths (scale ratios, anchoring, the virtualizer's model)
//! keeps raw values — the snap is a presentation concern, and at under one
//! device pixel per page the rounding never accumulates visibly.

/// Round `v` (CSS px) to the nearest whole device pixel for a display whose
/// device-pixel ratio is `dpr`. Split from [`snap_px`] so the arithmetic is
/// testable off the browser: the wasm-only half adds nothing but the ratio.
fn snap_to(v: f64, dpr: f64) -> f64 {
    // A non-finite or nonsensical ratio (some headless environments report 0)
    // would turn a good coordinate into NaN; pass the value through instead.
    if !(v.is_finite() && dpr.is_finite() && dpr > 0.0) {
        return v;
    }
    (v * dpr).round() / dpr
}

/// The display's current device-pixel ratio, defaulting to 1.0 off-browser.
/// Read live rather than cached: dragging to a second monitor or changing the
/// browser's zoom changes it without any event this module subscribes to.
/// `snap_to` above stays pure, so the host suite proves the rule without a
/// browser; only this read is wasm-only.
#[cfg(target_arch = "wasm32")]
fn device_pixel_ratio() -> f64 {
    web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .filter(|d| *d > 0.0 && d.is_finite())
        .unwrap_or(1.0)
}

/// The same answer with no display to ask: the grid is the CSS grid, so nothing
/// is snapped and nothing is harmed. Keeps the crate host-testable.
#[cfg(not(target_arch = "wasm32"))]
fn device_pixel_ratio() -> f64 {
    1.0
}

/// Snap a CSS-px length or offset to the device-pixel grid.
pub fn snap_px(v: f64) -> f64 {
    snap_to(v, device_pixel_ratio())
}

/// One device pixel, expressed in CSS px. Used by the no-gap layout to overlap
/// neighbouring pages by the smallest amount the compositor can resolve.
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
        // 1.25: the grid step is 0.8 CSS px, so a snapped value is always a
        // whole number of device pixels.
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
        // Two stacked pages: the second starts where the first ends. Snapped
        // independently, their shared edge must be the same device row — the
        // seam this module exists to remove.
        let dpr = 1.25;
        let h = snap_to(841.89, dpr);
        let top_of_second = snap_to(h, dpr);
        assert_eq!(top_of_second, h);
    }

    #[test]
    fn the_boundary_helper_is_the_ratio_the_display_reports() {
        // Off-browser the ratio is 1, so snapping must be the identity — the
        // host test run below is only meaningful if it is.
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
