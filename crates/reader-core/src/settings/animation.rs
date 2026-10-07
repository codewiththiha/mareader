//! What may animate, and what may not — the Animations tab's schema.

use serde::{Deserialize, Serialize};

use super::on_true;

/// What may animate, and what may not; `enabled` is the master.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnimationSettings {
    /// Every animation in the reader, switches or not.
    #[serde(default = "on_true")]
    pub enabled: bool,
    /// The rail's open and close motion, docked tween or floating fade.
    #[serde(default = "on_true")]
    pub sidebar_slide: bool,
    /// The page re-fits on every frame of a window drag, or once.
    #[serde(default = "on_true")]
    pub canvas_resize: bool,
    /// A zoom eases to its target instead of appearing there.
    #[serde(default = "on_true")]
    pub zoom: bool,
    /// Jumping to a page (or to a search hit) glides the column there.
    #[serde(default = "on_true")]
    pub scroll_jumps: bool,
}

impl Default for AnimationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sidebar_slide: true,
            canvas_resize: true,
            zoom: true,
            scroll_jumps: true,
        }
    }
}
