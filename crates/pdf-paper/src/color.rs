//! The tiny colour algebra the paper pipeline needs: an RGB triple, its hex
//! string (the wire format `--pdf-paper` speaks), and linear interpolation.

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

    /// Parse `#rrggbb`. Returns `None` for anything else — a malformed
    /// cached colour must be dropped, not guessed at.
    pub fn parse_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim();
        let bytes = hex.strip_prefix('#')?;
        if bytes.len() != 6 || !bytes.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |range: std::ops::Range<usize>| {
            u8::from_str_radix(&bytes[range], 16).ok()
        };
        Some(Self {
            r: channel(0..2)?,
            g: channel(2..4)?,
            b: channel(4..6)?,
        })
    }
}

/// Linear blend of two colours; `t` 0 returns `a`, 1 returns `b`, values
/// outside `0..=1` are clamped. Rounded per channel so repeated round trips
/// through the same pair stay stable.
pub fn lerp(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    Rgb::new(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let c = Rgb::new(0x40, 0xa0, 0xff);
        assert_eq!(c.to_hex(), "#40a0ff");
        assert_eq!(Rgb::parse_hex("#40a0ff"), Some(c));
        assert_eq!(Rgb::parse_hex("#40A0FF"), Some(c));
        assert_eq!(Rgb::parse_hex(" #40a0ff "), Some(c));
    }

    #[test]
    fn malformed_hex_is_rejected_not_guessed() {
        assert_eq!(Rgb::parse_hex("#40a0"), None);
        assert_eq!(Rgb::parse_hex("40a0ff"), None);
        assert_eq!(Rgb::parse_hex("#40a0zz"), None);
        assert_eq!(Rgb::parse_hex(""), None);
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
