//! What may animate, and what may not — the Animations tab's schema.

use serde::{Deserialize, Serialize};

use super::on_true;

/// What may animate, and what may not. `enabled` is the master, and it lives
/// in the Layout tab because it is a layout-scale decision; the detail
/// switches are the Animations tab, which — like every animation here — only
/// exists while the master is on. The projection that applies the master is
/// `Motion::from_prefs` in the app crate, so no consumer asks twice.
///
/// Turning a switch off never skips the change itself: it renders the END
/// frame instantly — the zoom still lands at its target, the page still gets
/// the new width, the column still arrives where it was sent. Nothing is
/// lost, only interpolated, which is what makes the group safe to freeze.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnimationSettings {
    /// Every animation in the reader, including the ones with no switch of
    /// their own (menu pops, toasts, the theme cross-fade, the title bar's
    /// auto-hide). Off, the Animations tab is not offered either.
    #[serde(default = "on_true")]
    pub enabled: bool,
    /// The rail animates its open and close — the DOCKED rail tweens its
    /// width, the FLOATING rail fades (a transform slide would travel under
    /// the native traffic lights, which can only appear and disappear). The
    /// page ALWAYS rides the docked tween: following a measured container is
    /// not an animation, it is the resize, and deferring it would show a page
    /// cropped at the old scale for the whole settle window. A WINDOW drag is
    /// the burst with its own switch below: there, skipping frames is the
    /// point, because the alternative is a relayout plus a raster per drag
    /// frame.
    #[serde(default = "on_true")]
    pub sidebar_slide: bool,
    /// The page re-fits on every frame of a window drag. Off, it re-fits once,
    /// when the drag stops.
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
