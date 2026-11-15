use mareader_appearance::{BaseMode, FULL_TINT, NoiseMode};

use crate::icons::Icon;

/// Share of `FULL_TINT` in percent (`0..=100`).
pub(crate) fn strength_percent(strength: u8) -> u8 {
    let whole = u16::from(FULL_TINT);
    let share = u16::from(strength.min(FULL_TINT)) * 100 / whole;
    u8::try_from(share).unwrap_or(100)
}

pub(crate) fn noise_label(n: NoiseMode) -> &'static str {
    match n {
        NoiseMode::Off => "Off",
        NoiseMode::Static => "Static",
        NoiseMode::Animated => "Animated",
    }
}

pub(crate) fn mode_icon(b: BaseMode) -> Icon {
    match b {
        BaseMode::Light => Icon::Light,
        BaseMode::Dark => Icon::Dark,
        BaseMode::Dim => Icon::Dim,
        BaseMode::Black => Icon::Black,
    }
}
