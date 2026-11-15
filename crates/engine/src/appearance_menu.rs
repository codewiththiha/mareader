//! The quick text dials the appearance popover offers, and the tint's maths.

use mareader_appearance::FULL_TINT;
use mareader_settings::TextSettings;

/// Picking a hue while untinted lifts strength here so the choice shows.
pub const STRENGTH_ON_HUE: u8 = 18;

/// Strength as a share of the whole tint, so all its travel shows.
pub fn strength_percent(strength: u8) -> u8 {
    let whole = u16::from(FULL_TINT);
    let share = u16::from(strength.min(FULL_TINT)) * 100 / whole;
    u8::try_from(share).unwrap_or(100)
}

/// The share back as a strength, rounded to the nearest one.
pub fn percent_strength(percent: u8) -> u8 {
    let whole = u16::from(FULL_TINT);
    let strength = (u16::from(percent.min(100)) * whole + 50) / 100;
    u8::try_from(strength).unwrap_or(FULL_TINT)
}

/// The text dials the popover offers for a book, from its text settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuickText {
    pub size_pct: f32,
    pub line_height: f32,
    pub paragraph_margin: f32,
    pub override_font: bool,
    pub override_layout: bool,
}

impl QuickText {
    pub fn of(t: &TextSettings) -> Self {
        Self {
            size_pct: t.size_pct(),
            line_height: t.line_height,
            paragraph_margin: t.paragraph_margin,
            override_font: t.override_font,
            override_layout: t.override_layout,
        }
    }

    /// Writes the dials back; an untouched size keeps its exact points.
    pub fn apply(self, t: &mut TextSettings) {
        if self.size_pct != t.size_pct() {
            t.set_size_pct(self.size_pct);
        }
        t.line_height = self.line_height;
        t.paragraph_margin = self.paragraph_margin;
        t.override_font = self.override_font;
        t.override_layout = self.override_layout;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_reads_as_a_share_of_the_whole_tint() {
        assert_eq!(strength_percent(0), 0);
        assert_eq!(strength_percent(FULL_TINT), 100);
        assert_eq!(strength_percent(FULL_TINT / 2), 50);
        assert_eq!(strength_percent(STRENGTH_ON_HUE), 36);
        // Past the whole tint nothing changes, so the dial stays full.
        assert_eq!(strength_percent(100), 100);
        for percent in (0..=100).step_by(2) {
            assert_eq!(strength_percent(percent_strength(percent)), percent);
        }
    }
}
