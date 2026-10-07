//! The tiny colour algebra the paper pipeline needs.
use serde::{Deserialize, Serialize};

/// A straight RGB colour. No alpha: paper is opaque by definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `#rrggbb`, lowercase — the form the engine publishes as
    /// `--pdf-paper` and caches per document path.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// Linear blend of two colours, clamped.
pub fn lerp(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    Rgb::new(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_lowercase_and_zero_padded() {
        assert_eq!(Rgb::new(0x40, 0xa0, 0xff).to_hex(), "#40a0ff");
        // The wire format both the engine and the CSS property expect.
        assert_eq!(Rgb::new(0x00, 0x0a, 0xff).to_hex(), "#000aff");
    }

    #[test]
    fn lerp_hits_both_ends_and_the_midpoint() {
        let a = Rgb::new(0x40, 0x40, 0x40);
        let b = Rgb::new(0xff, 0xff, 0xff);
        assert_eq!(lerp(a, b, 0.0), a);
        assert_eq!(lerp(a, b, 1.0), b);
        // (64 + 255) / 2 = 159.5 → 160 → #a0
        assert_eq!(lerp(a, b, 0.5), Rgb::new(0xa0, 0xa0, 0xa0));
    }

    #[test]
    fn lerp_clamps_out_of_range_t() {
        let a = Rgb::new(0, 0, 0);
        let b = Rgb::new(255, 255, 255);
        assert_eq!(lerp(a, b, -1.0), a);
        assert_eq!(lerp(a, b, 2.0), b);
    }
}
