//! Appearance: base mode, colour tint, texture, noise — and the maths that
//! turns them into CSS values.
//!
//!   * `model`      — the data model (modes + [`Appearance`]), the persisted schema
//!   * `base`       — the raw palettes behind each base mode
//!   * `presets`    — the built-in looks, and the user's own saved ones
//!   * `shared`     — the kernel both pipelines consume: the OKLCH maths, the
//!     tint hue mapping and ceilings, the noise/texture helpers
//!   * `preview`    — the preset-thumbnail preview style/class
//!   * `raster`     — the filter chain + UI-token overrides for pages that
//!     arrive as bitmaps
//!   * `reflowable` — the direct-colour palette for pages painted as CSS text
//!
//! The two pipelines read the model and the shared kernel; neither reads the
//! other, and nothing outside this tree knows which of them a page went
//! through.

pub(crate) mod base;
mod model;
pub mod presets;
pub(crate) mod preview;
pub mod raster;
pub mod reflowable;
pub mod shared;

pub use model::{Appearance, BaseMode, NoiseMode, TextureMode};

/// One field an appearance slider is allowed to live-edit, carried by the
/// theme handle's scrub callback so both sides of the boundary agree on
/// which dial is moving (the canvas-scrub gate asks it of a tint). A plain
/// enum on purpose: this crate stays free of wasm and leptos.
#[derive(Debug, Clone, Copy)]
pub enum AppearanceScrub {
    Tint { hue: u16, strength: u8 },
    TextureOpacity(u8),
    TextureScale(u16),
    NoiseIntensity(u8),
}

impl AppearanceScrub {
    /// Apply this patch to an appearance and clamp its ranges. The one
    /// writer of slider values — a preview paint and its delayed commit
    /// both land here, so the two can never disagree on a mapping.
    pub fn apply(self, a: &mut Appearance) {
        match self {
            Self::Tint { hue, strength } => {
                a.tint_hue = hue;
                a.tint_strength = strength;
            }
            Self::TextureOpacity(v) => a.texture_opacity = v,
            Self::TextureScale(v) => a.texture_scale = v,
            Self::NoiseIntensity(v) => a.noise_intensity = v,
        }
        a.sanitize();
    }
}

/// Fixtures the appearance tests share. Every pipeline test starts from the
/// same [`Appearance`] with only the tint dial set, and every assertion reads
/// a colour back out of an emitted string — both were written per module (five
/// copies of `tinted`, four hand-rolled oklch parsers). The reader here goes
/// through [`parse_color`], the production parser: a test that parses colours
/// with its own private grammar can agree with itself while the real one
/// disagrees.
#[cfg(test)]
pub(crate) mod fixture {
    use super::shared::oklch::parse_color;
    use super::{Appearance, BaseMode};

    /// An appearance with only the tint dial set; everything else default.
    pub(crate) fn tinted(base: BaseMode, hue: u16, strength: u8) -> Appearance {
        Appearance {
            base,
            tint_hue: hue,
            tint_strength: strength,
            ..Default::default()
        }
    }

    /// (L, C, H) of an emitted colour literal.
    pub(crate) fn lch(value: &str) -> (f64, f64, f64) {
        parse_color(value).unwrap_or_else(|| panic!("not a colour this reader emits: {value}"))
    }
}
