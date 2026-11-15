//! Which desktop's window conventions to follow; tests can override it.

use crate::widgets::CaptionStyle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Native traffic lights over a full-size content view.
    Mac,
    /// Frameless, Windows-style captions.
    Windows,
    /// Frameless, GNOME-style captions.
    Gnome,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Gnome
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "mac" | "macos" => Some(Self::Mac),
            "windows" => Some(Self::Windows),
            "gnome" | "linux" => Some(Self::Gnome),
            _ => None,
        }
    }

    /// Room the title bar leaves at its left edge: macOS's traffic lights.
    pub fn leading_inset(self) -> f32 {
        match self {
            Self::Mac => 80.0,
            Self::Windows | Self::Gnome => 8.0,
        }
    }

    /// The caption buttons the app draws; macOS draws its own.
    pub fn captions(self) -> Option<CaptionStyle> {
        match self {
            Self::Mac => None,
            Self::Windows => Some(CaptionStyle::Windows),
            Self::Gnome => Some(CaptionStyle::Gnome),
        }
    }

    /// True when the app draws caption buttons and resize edges itself.
    pub fn frameless(self) -> bool {
        !matches!(self, Self::Mac)
    }
}

/// Where macOS's window buttons go: with a drawn rail, else with the bar.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NativeLights {
    /// Points left (negative) of their resting place.
    pub shift: f32,
    /// 0 hides them; they can't be clicked then.
    pub opacity: f32,
}

/// Where AppKit drew its buttons, in egui points, for backing them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightSpots {
    /// The close button's centre with no shift, from the window's top left.
    pub first: egui::Pos2,
    /// From one centre to the next.
    pub pitch: f32,
    pub size: f32,
    /// macOS 26's glass buttons, whose white sheen fades into a light bar.
    pub glass: bool,
}

impl LightSpots {
    /// macOS 26's buttons as the desktop binary places them, for off a Mac.
    pub const STAND_IN: Self = Self {
        first: egui::pos2(27.0, crate::theme::TITLE_BAR_H / 2.0),
        pitch: 23.0,
        size: 14.0,
        glass: true,
    };

    /// Each button's centre, `shift` points from rest.
    pub fn centres(&self, shift: f32) -> [egui::Pos2; 3] {
        [0.0, 1.0, 2.0].map(|i| self.first + egui::vec2(shift + i * self.pitch, 0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lights_slide_with_their_shift() {
        let [close, _, zoom] = LightSpots::STAND_IN.centres(-10.0);
        assert_eq!(close, egui::pos2(17.0, 24.0));
        assert_eq!(zoom.x - close.x, 46.0);
    }
}
