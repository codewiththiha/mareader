//! The study-mode switch and its dials, shared with the pure rules.

use serde::{Deserialize, Serialize};

/// Which study switch the reader is on, with its batch and deck dials.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudyModeSettings {
    pub mode: mareader_study::Mode,
    #[serde(flatten)]
    pub dials: mareader_study::Cfg,
}

impl Default for StudyModeSettings {
    fn default() -> Self {
        Self {
            mode: mareader_study::Mode::Off,
            dials: mareader_study::Cfg::default(),
        }
    }
}

impl StudyModeSettings {
    pub fn sanitize(&mut self) {
        self.dials.sanitize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_dials_clamp_on_sanitize() {
        let mut s = StudyModeSettings {
            mode: mareader_study::Mode::Proactive,
            dials: mareader_study::Cfg {
                pages: 99,
                max_cards: 1,
                ..mareader_study::Cfg::default()
            },
        };
        s.sanitize();
        assert_eq!(s.mode, mareader_study::Mode::Proactive);
        assert_eq!(s.dials.pages, 10);
        assert_eq!(s.dials.max_cards, 4);
    }

    #[test]
    fn mode_defaults_to_off() {
        let s = StudyModeSettings::default();
        assert_eq!(s.mode, mareader_study::Mode::Off);
        assert_eq!(s.dials.sec_per_card, 6);
    }
}
