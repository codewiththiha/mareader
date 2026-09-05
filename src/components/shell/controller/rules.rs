//! The shell's layout rules, as pure functions.
//!
//! Every question the chrome asks about the rail — is it still painted, is this
//! panel showing, are the thumbnail cells alive — is answered by the controller
//! ([`super::ShellController`]), and every answer it gives comes from here. The
//! two are split because the questions are pure and the controller is not: a
//! rule is four values in and a bool out, with no signal, no owner and no
//! animation state to set up, so the cases that matter can simply be written
//! down and asserted.
//!
//! That is worth more here than it usually is, because these rules describe a
//! MOTION. The rail's raw mode flips to `None` on the click that closes it,
//! before the rail is out of the way, so "is the rail present" cannot be
//! "is the mode open" — chrome that believed the mode would let go of the
//! title bar's inset on frame one of a 300ms slide, and the traffic lights
//! would jump while the rail was still moving under them. Every rule below
//! takes the collapsing flag for that reason, and the tests are the cases a
//! reader would otherwise have to reason out from the animation.

use crate::state::SidebarMode;

/// Whether the rail is still painted and therefore still owns title-bar
/// chrome space. Stays true through the close slide after the raw mode has
/// changed to `None`.
pub(super) fn sidebar_is_present(mode: SidebarMode, collapsing: bool) -> bool {
    mode != SidebarMode::None || collapsing
}

/// Whether `panel` should stay painted this frame (see
/// [`super::ShellController::panel_shown`]).
pub(super) fn panel_is_shown(panel: SidebarMode, mode: SidebarMode, collapsing: bool, last: SidebarMode) -> bool {
    mode == panel || (mode == SidebarMode::None && collapsing && last == panel)
}

/// Final mount gate for thumbnail cells. Even if the panel state would
/// normally preserve cells through an outro, there is nothing to preserve
/// when a close arrives before the opening delay created any cells.
pub(super) fn thumbnail_cells_are_live(
    cells_mounted: bool,
    mode: SidebarMode,
    collapsing: bool,
    last: SidebarMode,
) -> bool {
    cells_mounted && thumbs_should_stay_mounted(mode, collapsing, last)
}

/// Whether the thumbnail grid should keep its cells mounted.
///
/// Mounted while Thumbs is showing, while Outline is showing (so a tab
/// switch does not re-render every thumb), and while the Thumbs panel is
/// mid-outro. Dropped only once a close from Thumbs has finished — that
/// is what releases the live canvases without a quick-reopen spike.
fn thumbs_should_stay_mounted(mode: SidebarMode, collapsing: bool, last: SidebarMode) -> bool {
    match mode {
        SidebarMode::Thumbs | SidebarMode::Outline => true,
        SidebarMode::None => collapsing && last == SidebarMode::Thumbs,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        panel_is_shown, sidebar_is_present, thumbnail_cells_are_live, thumbs_should_stay_mounted,
    };
    use crate::state::SidebarMode;

    #[test]
    fn a_close_keeps_the_open_panel_painted_until_the_motion_ends() {
        // Frame one of a Thumbs close: still painted, so it can fade/clip.
        assert!(panel_is_shown(
            SidebarMode::Thumbs,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs
        ));
        assert!(!panel_is_shown(
            SidebarMode::Outline,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs
        ));
        // After the slide: both hidden.
        assert!(!panel_is_shown(
            SidebarMode::Thumbs,
            SidebarMode::None,
            false,
            SidebarMode::Thumbs
        ));
    }

    #[test]
    fn chrome_space_is_held_until_the_close_motion_lands() {
        assert!(sidebar_is_present(SidebarMode::Thumbs, false));
        // Frame one of a close: raw mode is None, but the rail is still
        // sliding or fading and title-bar chrome must remain aligned with it.
        assert!(sidebar_is_present(SidebarMode::None, true));
        assert!(!sidebar_is_present(SidebarMode::None, false));
    }

    #[test]
    fn a_tab_switch_shows_only_the_active_panel() {
        assert!(panel_is_shown(
            SidebarMode::Outline,
            SidebarMode::Outline,
            false,
            SidebarMode::Outline
        ));
        assert!(!panel_is_shown(
            SidebarMode::Thumbs,
            SidebarMode::Outline,
            false,
            SidebarMode::Outline
        ));
    }

    #[test]
    fn thumbnail_cells_require_a_real_mount() {
        // The helper remains defensive: an outro state alone never creates
        // cells; the open transition is what mounts them.
        assert!(!thumbnail_cells_are_live(
            false,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs,
        ));
        // Once cells genuinely exist, the ordinary open and outro paths keep
        // working as before.
        assert!(thumbnail_cells_are_live(
            true,
            SidebarMode::Thumbs,
            false,
            SidebarMode::Thumbs,
        ));
        assert!(thumbnail_cells_are_live(
            true,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs,
        ));
    }

    #[test]
    fn thumbs_stay_mounted_across_a_tab_switch_but_not_a_finished_close() {
        // Instant Thumbs ↔ Outline: keep the canvases.
        assert!(thumbs_should_stay_mounted(
            SidebarMode::Outline,
            false,
            SidebarMode::Outline
        ));
        assert!(thumbs_should_stay_mounted(
            SidebarMode::Thumbs,
            false,
            SidebarMode::Thumbs
        ));
        // Mid-outro from Thumbs: keep them so a quick reopen is free.
        assert!(thumbs_should_stay_mounted(
            SidebarMode::None,
            true,
            SidebarMode::Thumbs
        ));
        // Slide finished, or we closed from Outline: drop the live canvases.
        assert!(!thumbs_should_stay_mounted(
            SidebarMode::None,
            false,
            SidebarMode::Thumbs
        ));
        assert!(!thumbs_should_stay_mounted(
            SidebarMode::None,
            true,
            SidebarMode::Outline
        ));
    }
}
