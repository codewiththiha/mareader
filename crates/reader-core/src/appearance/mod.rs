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

pub mod base;
mod model;
pub mod presets;
pub(crate) mod preview;
pub mod raster;
pub mod reflowable;
pub mod shared;

pub use model::{Appearance, BaseMode, NoiseMode, TextureMode};

/// Fixtures the appearance tests share.
///
/// Every pipeline test starts from the same place — an [`Appearance`] with
/// only the tint dial set — and every assertion reads a colour back out of an
/// emitted string. Both were written per module (five copies of `tinted`, four
/// hand-rolled `oklch(` parsers), so a change to the model or to the emitted
/// format meant finding them all. The reader here goes through
/// [`parse_color`], the production parser, which is the point: a test that
/// parses a colour with its own private grammar can agree with itself while
/// the real one disagrees.
#[cfg(test)]
pub(crate) mod fixture {
    use super::shared::oklch::parse_color;
    use super::{Appearance, BaseMode};

    /// An appearance with only the tint dial set; everything else default.
    pub(crate) fn tinted(base: BaseMode, hue: u16, strength: u8) -> Appearance {
        Appearance { base, tint_hue: hue, tint_strength: strength, ..Default::default() }
    }

    /// (L, C, H) of an emitted colour literal.
    pub(crate) fn lch(value: &str) -> (f64, f64, f64) {
        parse_color(value).unwrap_or_else(|| panic!("not a colour this reader emits: {value}"))
    }
}

