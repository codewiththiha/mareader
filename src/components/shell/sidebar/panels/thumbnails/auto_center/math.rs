//! The pure timing rules of the auto-center machinery.
//!
//! No effects, no listeners, no DOM: every decision the glide makes is a
//! named function here, so the timing rules are unit-tested rather than
//! buried in closures. The effect / listener installation that consumes
//! them lives in [`super::wiring`].

use virtual_list_leptos::Virtualizer;

use super::super::geometry::{CELL_W, GLIDE_DEBOUNCE_MS, GRACE_MS};

/// Content-coordinate offset that vertically centers a row of height
/// `cell_h` whose top sits at `row_top`, in a viewport `vh` tall.
pub(super) fn center_offset(row_top: f64, cell_h: f64, vh: f64) -> Option<f64> {
    (vh > 0.0).then(|| row_top + cell_h / 2.0 - vh / 2.0)
}

/// Content-coordinate target that vertically centers `page`'s cell.
pub(super) fn center_target(v: &Virtualizer, page: u32, aspect: f64, vh: f64) -> Option<f64> {
    if page == 0 {
        return None;
    }
    let idx = (page - 1) as usize;
    center_offset(v.offset_of(idx), CELL_W * aspect, vh)
}

/// Initial delay before an armed glide fires.
///
/// Inside the after-drive grace the glide waits out the remainder plus a
/// beat (so a panel the reader just flicked doesn't start sliding under
/// them); outside it, the plain debounce applies. The open path never
/// reaches this — it snaps in [`super::wiring::snap_to_page`] — so there is
/// no "just opened" delay here.
pub(super) fn glide_delay(in_grace_remaining_ms: Option<f64>) -> u64 {
    match in_grace_remaining_ms {
        Some(remaining) => (GRACE_MS - remaining + 60.0) as u64,
        None => GLIDE_DEBOUNCE_MS,
    }
}

/// One tick of the armed glide: what to do, given what changed since arming.
#[derive(Debug)]
pub(super) enum GlideVerdict {
    /// The panel closed, the page moved on, or the target is already
    /// centered (within a px): cancel and drop the timer.
    Cancel,
    /// The reader drove the panel less than a grace period ago: hold for
    /// this many ms, then run the step again.
    Hold(u64),
    /// Fire the glide onto this content offset.
    Fire(f64),
}

pub(super) fn glide_verdict(
    in_thumbs: bool,
    page_now: u32,
    armed_page: u32,
    target: Option<f64>,
    current: f64,
    since_drive_ms: f64,
) -> GlideVerdict {
    if !in_thumbs
        || page_now != armed_page
        || target.is_none_or(|t| (t - current).abs() <= 1.0)
    {
        return GlideVerdict::Cancel;
    }
    if since_drive_ms < GRACE_MS {
        return GlideVerdict::Hold((GRACE_MS - since_drive_ms + 50.0) as u64);
    }
    GlideVerdict::Fire(target.unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_offset_places_the_row_mid_viewport() {
        // A 100px row whose top sits at 200, in a 500px viewport: the scroll
        // offset that shows it centered is 200 + 50 - 250 = 0.
        assert_eq!(center_offset(200.0, 100.0, 500.0), Some(0.0));
        // Same row further down: the offset moves with the row's midpoint.
        assert_eq!(center_offset(1000.0, 100.0, 500.0), Some(800.0));
        // Unmeasured viewport: nothing to center against.
        assert_eq!(center_offset(200.0, 100.0, 0.0), None);
    }

    #[test]
    fn glide_delay_waits_out_the_grace_otherwise_debounces() {
        // Deep inside the grace: remainder plus the 60ms beat.
        assert_eq!(glide_delay(Some(100.0)), (GRACE_MS - 100.0 + 60.0) as u64);
        // At the grace edge the beat alone remains.
        assert_eq!(glide_delay(Some(GRACE_MS)), 60);
        // No recent drive: the plain debounce.
        assert_eq!(glide_delay(None), GLIDE_DEBOUNCE_MS);
    }

    #[test]
    fn glide_verdict_cancels_when_the_world_moved_on() {
        let target = Some(1000.0);
        // Panel closed.
        assert!(matches!(
            glide_verdict(false, 3, 3, target, 0.0, GRACE_MS * 2.0),
            GlideVerdict::Cancel
        ));
        // Page changed under us.
        assert!(matches!(
            glide_verdict(true, 4, 3, target, 0.0, GRACE_MS * 2.0),
            GlideVerdict::Cancel
        ));
        // Already centered within a pixel.
        assert!(matches!(
            glide_verdict(true, 3, 3, target, 999.5, GRACE_MS * 2.0),
            GlideVerdict::Cancel
        ));
        // Target gone (viewport unmeasured).
        assert!(matches!(
            glide_verdict(true, 3, 3, None, 0.0, GRACE_MS * 2.0),
            GlideVerdict::Cancel
        ));
    }

    #[test]
    fn glide_verdict_holds_inside_the_grace_and_fires_past_it() {
        // Inside the grace: hold for the remainder plus the 50ms beat.
        match glide_verdict(true, 3, 3, Some(1000.0), 0.0, 100.0) {
            GlideVerdict::Hold(wait) => assert_eq!(wait, (GRACE_MS - 100.0 + 50.0) as u64),
            other => panic!("expected Hold, got {other:?}"),
        }
        // Past the grace: fire onto the target.
        match glide_verdict(true, 3, 3, Some(1000.0), 0.0, GRACE_MS + 1.0) {
            GlideVerdict::Fire(t) => assert_eq!(t, 1000.0),
            other => panic!("expected Fire, got {other:?}"),
        }
    }
}
