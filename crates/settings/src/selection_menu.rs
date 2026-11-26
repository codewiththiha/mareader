//! Global preferred size of the selection menu, in logical points.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectionMenuSize {
    pub width: u16,
    pub height: u16,
}

impl SelectionMenuSize {
    pub const WIDTH: (u16, u16) = (180, 360);
    pub const HEIGHT: (u16, u16) = (160, 480);

    pub fn sanitize(&mut self) {
        self.width = self.width.clamp(Self::WIDTH.0, Self::WIDTH.1);
        self.height = self.height.clamp(Self::HEIGHT.0, Self::HEIGHT.1);
    }
}

impl Default for SelectionMenuSize {
    fn default() -> Self {
        Self {
            width: 216,
            height: 216,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_size_round_trips_and_is_clamped() {
        let mut settings = crate::Settings::default();
        settings.study.selection_menu = SelectionMenuSize {
            width: 320,
            height: 360,
        };
        assert_eq!(
            crate::load(&crate::to_json(&settings))
                .settings
                .study
                .selection_menu,
            settings.study.selection_menu
        );
        settings.study.selection_menu = SelectionMenuSize {
            width: 1,
            height: u16::MAX,
        };
        settings.sanitize();
        assert_eq!(
            settings.study.selection_menu.width,
            SelectionMenuSize::WIDTH.0
        );
        assert_eq!(
            settings.study.selection_menu.height,
            SelectionMenuSize::HEIGHT.1
        );
    }
}
