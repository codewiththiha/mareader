//! Grain mode -> the `body` classes and the opacity the noise overlay keys
//! off. Shared by design: the grain is a body-level layer every format's
//! page sits under, and no format pipeline gets to own it.

use crate::appearance::{Appearance, NoiseMode};

/// The two `body` classes the grain overlay's CSS keys off, as `(name, on)`.
///
/// `noise-enabled` shows the layer; `noise-animated` makes it crawl (it
/// re-seeds the pattern every frame like real film grain). Off clears both,
/// Static enables the layer, Animated enables both.
pub fn body_class_state(mode: NoiseMode) -> [(&'static str, bool); 2] {
    [
        ("noise-enabled", mode.is_on()),
        ("noise-animated", matches!(mode, NoiseMode::Animated)),
    ]
}

/// `--noise-opacity`, the grain strength dial as 0..=1. Written on `body`,
/// where the overlay resolves it.
pub fn css_vars(a: &Appearance) -> Vec<(&'static str, String)> {
    vec![(
        "--noise-opacity",
        format!("{}", a.noise_intensity.min(100) as f64 / 100.0),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_class_pair_tracks_the_mode() {
        assert_eq!(body_class_state(NoiseMode::Off), [("noise-enabled", false), ("noise-animated", false)]);
        assert_eq!(body_class_state(NoiseMode::Static), [("noise-enabled", true), ("noise-animated", false)]);
        assert_eq!(body_class_state(NoiseMode::Animated), [("noise-enabled", true), ("noise-animated", true)]);
    }

    #[test]
    fn the_opacity_var_is_a_unit_fraction() {
        let a = Appearance { noise_intensity: 65, ..Default::default() };
        let vars = css_vars(&a);
        assert_eq!(vars[0].0, "--noise-opacity");
        assert_eq!(vars[0].1, "0.65");
    }
}
