//! Appearance: base mode, tint, texture and noise, and the maths that
//! turns them into CSS.

pub(crate) mod base;
mod model;
pub mod presets;
pub(crate) mod preview;
pub mod raster;
pub mod reflowable;
pub mod shared;

pub use model::{Appearance, BaseMode, NoiseMode, TextureMode};

/// One field an appearance slider may live-edit.
#[derive(Debug, Clone, Copy)]
pub enum AppearanceScrub {
    Tint { hue: u16, strength: u8 },
    TextureOpacity(u8),
    TextureScale(u16),
    NoiseIntensity(u8),
}

impl AppearanceScrub {
    /// Apply this patch to an appearance and clamp its ranges.
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

/// Fixtures the appearance tests share: an appearance and colour
/// readback.
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
