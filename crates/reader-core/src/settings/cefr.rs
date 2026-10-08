//! The vocabulary highlighter's knobs, riding on `Settings` as flat
//! `cefr_*` fields.

use serde::{Deserialize, Serialize};

/// The lowest selectable band; A1 would highlight nearly every word.
pub const MIN_CEFR_BAND: u8 = 2;

/// The slider stop: words whose band is STRICTLY higher are highlighted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CefrLevel {
    A2,
    B1,
    #[default]
    B2,
    C1,
    C2,
}

impl CefrLevel {
    /// The stops in slider order.
    pub const ALL: &'static [CefrLevel] = &[Self::A2, Self::B1, Self::B2, Self::C1, Self::C2];

    pub fn all() -> &'static [CefrLevel] {
        Self::ALL
    }

    /// The 2..=6 band this stop stands for (A1 would be 1, and is not
    /// offered).
    pub fn band(self) -> u8 {
        match self {
            Self::A2 => 2,
            Self::B1 => 3,
            Self::B2 => 4,
            Self::C1 => 5,
            Self::C2 => 6,
        }
    }

    /// The stop a 2..=6 band answers, clamped into range.
    pub fn from_band(band: u8) -> Self {
        match band.clamp(MIN_CEFR_BAND, 6) {
            3 => Self::B1,
            4 => Self::B2,
            5 => Self::C1,
            6 => Self::C2,
            _ => Self::A2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::A2 => "A2",
            Self::B1 => "B1",
            Self::B2 => "B2",
            Self::C1 => "C1",
            Self::C2 => "C2",
        }
    }

    /// What the slider's live readout says: the stop and what it marks.
    pub fn detail(self) -> &'static str {
        match self {
            Self::A2 => "Highlights B1 and above",
            Self::B1 => "Highlights B2 and above",
            Self::B2 => "Highlights C1 and above",
            Self::C1 => "Highlights C2",
            Self::C2 => "Nothing left above C2",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_persisted_schema_is_snake_case() {
        assert_eq!(serde_json::to_string(&CefrLevel::B2).unwrap(), "\"b2\"");
        assert_eq!(
            serde_json::from_str::<CefrLevel>("\"c1\"").unwrap(),
            CefrLevel::C1
        );
    }

    #[test]
    fn bands_and_stops_agree_in_both_directions() {
        for stop in CefrLevel::all() {
            assert_eq!(CefrLevel::from_band(stop.band()), *stop);
        }
        // A stray saved band clamps instead of panicking.
        assert_eq!(CefrLevel::from_band(1), CefrLevel::A2);
        assert_eq!(CefrLevel::from_band(9), CefrLevel::C2);
        assert_eq!(CefrLevel::from_band(0), CefrLevel::A2);
    }

    #[test]
    fn the_default_stop_is_b2() {
        assert_eq!(CefrLevel::default(), CefrLevel::B2);
    }
}
