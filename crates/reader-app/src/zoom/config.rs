//! Zoom behaviour knobs in one place: the clamped scale range and the
//! animation profile. Every view mode shares one profile.

use reader_core::zoom_math::{MAX_SCALE, MIN_SCALE};

/// Duration of the zoom tween, in milliseconds. Linear, not eased — see
/// `animation.rs` for why the commit seam must not decelerate. 120ms keeps a
/// manual step feeling immediate while still reading as motion.
const ZOOM_ANIM_MS: f64 = 120.0;

/// How long an item evicted by ORDINARY SCROLLING stays mounted after it
/// leaves the window, milliseconds. Applied where the strips are built
/// (their virtualizers opt into retention with this grace); a zoom
/// transaction raises it to `ZOOM_GRACE_MS` for its duration.
pub const STRIP_SCROLL_GRACE_MS: u32 = 120;

/// How long an item evicted by a ZOOM COMMIT stays mounted, milliseconds.
/// Deliberately longer than the tween: the commit reinstalls geometry, the
/// window jumps, and the pages it evicts are still on screen — the grace
/// outlives the animation so the old surface never vanishes before the new
/// geometry stabilises.
pub const ZOOM_GRACE_MS: u32 = 300;

/// Ceiling on simultaneously retained (zombie) items per virtualizer. The
/// bridge is bounded or it would stop being virtualization.
pub const MAX_ZOMBIES: usize = 12;

/// How long the space around the page must be quiet before a container follow
/// commits its crisp render, milliseconds. The layout follows a sidebar slide
/// or window drag frame by frame; the rasters wait for the burst's end, so a
/// slide costs one render pass at the settled size instead of one per frame.
/// The same window doubles as the pause a fit-driven refit waits for after a
/// page turn, where following the layout per frame would mean zooming at every
/// row boundary of a mixed-size book.
pub const FOLLOW_SETTLE_MS: u64 = 180;

/// Scales closer than this are the same scale. One margin for the whole
/// pipeline: the resolver uses it to call a boundary step a no-op and the
/// coordinator to decline a transition that would not move. Two numbers would
/// mean a step one layer considers settled and the other animates.
pub(crate) const SETTLED_EPSILON: f64 = 0.0005;

/// How (and whether) a zoom animates. Public because it is a field of the
/// public [`ZoomProfile`]: a profile is a value a caller can hold and compare,
/// and a field whose type it cannot name is a profile it cannot construct.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomAnimationConfig {
    pub enabled: bool,
    pub duration_ms: f64,
}

/// How evicted virtual items are bridged across a window change. Public for
/// the same reason [`ZoomAnimationConfig`] is: it is a field of a public
/// struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoomRetentionConfig {
    /// Grace period items evicted by a zoom commit keep their DOM, in
    /// milliseconds. Should outlive the tween itself.
    pub grace_ms: u32,
    pub max_zombies: usize,
}

/// The zoom behaviour profile for one view mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomProfile {
    pub min: f64,
    pub max: f64,
    pub animation: ZoomAnimationConfig,
    pub retention: ZoomRetentionConfig,
}

impl ZoomProfile {
    /// Clamp a proposed scale into the profile's range. A non-finite input
    /// (NaN, infinity — a corrupt measurement upstream) collapses to the
    /// minimum rather than poisoning every derived geometry.
    pub fn clamp(&self, scale: f64) -> f64 {
        if !scale.is_finite() {
            return self.min;
        }
        scale.clamp(self.min, self.max)
    }

    /// Effective tween duration: zero when animation is disabled (the tween
    /// then degenerates to an immediate landing).
    pub fn duration_ms(&self) -> f64 {
        if self.animation.enabled {
            self.animation.duration_ms
        } else {
            0.0
        }
    }
}

/// The zoom profile. One for every view mode: the refactor that introduced
/// this config changed the zoom *architecture*, not the numbers.
pub fn zoom_profile() -> ZoomProfile {
    ZoomProfile {
        min: MIN_SCALE,
        max: MAX_SCALE,
        animation: ZoomAnimationConfig {
            enabled: true,
            duration_ms: ZOOM_ANIM_MS,
        },
        retention: ZoomRetentionConfig {
            grace_ms: ZOOM_GRACE_MS,
            max_zombies: MAX_ZOMBIES,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_honours_the_range_and_survives_garbage() {
        let p = zoom_profile();
        assert_eq!(p.clamp(0.01), MIN_SCALE);
        assert_eq!(p.clamp(999.0), MAX_SCALE);
        assert_eq!(p.clamp(1.25), 1.25);
        // NaN must not leak into geometry.
        assert_eq!(p.clamp(f64::NAN), MIN_SCALE);
    }

    #[test]
    fn a_disabled_animation_collapses_to_an_instant_landing() {
        let p = ZoomProfile {
            min: MIN_SCALE,
            max: MAX_SCALE,
            animation: ZoomAnimationConfig {
                enabled: false,
                duration_ms: 250.0,
            },
            retention: zoom_profile().retention,
        };
        assert_eq!(p.duration_ms(), 0.0);
        assert_eq!(zoom_profile().duration_ms(), ZOOM_ANIM_MS);
    }

    #[test]
    fn the_zoom_grace_outlives_the_tween() {
        // The commit's evictions must still be bridged after the animation
        // itself ends, or the old surface pops before the new geometry
        // stabilises.
        let p = zoom_profile();
        assert!(p.retention.grace_ms as f64 > p.duration_ms());
        assert!(p.retention.max_zombies > 0);
    }
}
