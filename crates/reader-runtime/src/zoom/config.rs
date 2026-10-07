//! Zoom knobs: the clamped scale range and the shared animation profile.

use reader_core::zoom_math::{MAX_SCALE, MIN_SCALE};

/// Tween duration, ms; linear on purpose (`animation.rs` explains the seam).
const ZOOM_ANIM_MS: f64 = 120.0;

/// Items a zoom commit evicts stay mounted this long (ms), outliving the tween.
pub const ZOOM_GRACE_MS: u32 = 300;

/// Ceiling on retained (zombie) items: past it, virtualization stops being one.
pub const MAX_ZOMBIES: usize = 12;

/// Quiet (ms) before a layout follow renders crisp; the fit-refit pause too.
pub const FOLLOW_SETTLE_MS: u64 = 180;

/// Scales closer than this are one scale; resolver and coordinator share it.
pub(crate) const SETTLED_EPSILON: f64 = 0.0005;

/// How a zoom animates; whether it does is `animation::interpolates`' call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomAnimationConfig {
    pub duration_ms: f64,
}

/// How evicted virtual items are bridged across a window change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoomRetentionConfig {
    /// Zombie grace for a zoom commit, ms; must outlive the tween.
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
    /// Clamp a proposed scale; a non-finite input collapses to the minimum.
    pub fn clamp(&self, scale: f64) -> f64 {
        if !scale.is_finite() {
            return self.min;
        }
        scale.clamp(self.min, self.max)
    }

    /// The tween's duration; zero commits in one discrete step.
    pub fn duration_ms(&self) -> f64 {
        self.animation.duration_ms
    }
}

/// The zoom profile every view mode shares.
pub fn zoom_profile() -> ZoomProfile {
    ZoomProfile {
        min: MIN_SCALE,
        max: MAX_SCALE,
        animation: ZoomAnimationConfig {
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
    fn the_profile_tweens_for_the_configured_duration() {
        assert_eq!(zoom_profile().duration_ms(), ZOOM_ANIM_MS);
    }

    #[test]
    fn the_zoom_grace_outlives_the_tween() {
        // Evictions must outlive the animation, or the old surface pops first.
        let p = zoom_profile();
        assert!(p.retention.grace_ms as f64 > p.duration_ms());
        assert!(p.retention.max_zombies > 0);
    }
}
