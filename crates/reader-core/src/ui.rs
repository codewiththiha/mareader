//! Shell-facing UI types the reader's core owns: the sidebar panel mode and
//! the motion projection.
//!
//! Both are plain data with no signal, no DOM and no format in sight — the
//! reader's state carries them, the shell's chrome asks about them, and both
//! builds (shell and reader) need to agree on them, which makes reader-core
//! their one home. Neither may grow behavior: a variant or flag here is a
//! fact the UI layers interpret, not a policy they delegate.

use crate::settings::AnimationSettings;

/// Which sidebar panel is open. UI chrome state, not viewer state:
/// reader-side rendering receives it as a plain signal when it needs to know
/// and never owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    None,
    Outline,
    Thumbs,
}

/// Which of the reader's motions animate. Projected from the persisted
/// [`AnimationSettings`] by the shell
/// (`effects::app::motion::publish_motion`) and read by everything that
/// moves a page, so no consumer has to know a master switch exists — the
/// projection already applied it.
///
/// Read TRACKED by views (the rail's transition class must change when the
/// reader flips a switch) and UNTRACKED by effects and scroll calls: a flag
/// that stops something animating must not be what triggers the animation.
///
/// Nothing here skips a change: off renders the end frame in the frame the
/// change arrives, which is why freezing the reader loses no fit, no follow
/// and no scroll target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Motion {
    /// The rail animates its open/close: the docked rail tweens its width
    /// (`SIDEBAR_SLIDE_MS`), the floating rail fades (`SIDEBAR_FADE_MS`).
    pub sidebar_slide: bool,
    /// The page rides a window drag: the canvas flexes on every frame of it.
    /// Riding the RAIL is not in here on purpose — a measured container is
    /// answered in the same frame, animation or not, and deferring it cropped
    /// the page for a visible instant.
    pub canvas_resize: bool,
    /// A zoom eases to its target over the profile's duration.
    pub zoom: bool,
    /// A jump to a page glides the column (or the thumbnail rail) over it.
    pub scroll_glide: bool,
}

impl Motion {
    /// The one place the master switch is honoured. Off, no detail can bring
    /// an animation back on; the Animations tab hides itself for the same
    /// reason, so the detail switches are never shown lying.
    pub const fn from_prefs(p: &AnimationSettings) -> Self {
        Self {
            sidebar_slide: p.enabled && p.sidebar_slide,
            canvas_resize: p.enabled && p.canvas_resize,
            zoom: p.enabled && p.zoom,
            scroll_glide: p.enabled && p.scroll_jumps,
        }
    }
}

impl Default for Motion {
    /// Everything moves. The shell publishes the reader's prefs before
    /// anything can act on them, and a reader that has not been published to
    /// yet (a document opening, a test) must not look broken.
    fn default() -> Self {
        Self {
            sidebar_slide: true,
            canvas_resize: true,
            zoom: true,
            scroll_glide: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_master_switch_freezes_every_detail() {
        let all_on = AnimationSettings::default();
        assert!(all_on.enabled);
        let m = Motion::from_prefs(&all_on);
        assert!(m.sidebar_slide && m.canvas_resize && m.zoom && m.scroll_glide);

        // With the master off, no detail can bring an animation back.
        let frozen = AnimationSettings {
            enabled: false,
            ..AnimationSettings::default()
        };
        assert!(frozen.zoom && frozen.sidebar_slide);
        let m = Motion::from_prefs(&frozen);
        assert!(!m.sidebar_slide);
        assert!(!m.canvas_resize);
        assert!(!m.zoom);
        assert!(!m.scroll_glide);
    }

    #[test]
    fn each_detail_owns_exactly_the_motion_it_names() {
        // `scroll_glide` is spelled `scroll_jumps` in the settings, so this
        // projection is the only place the two vocabularies meet — a crossed
        // wire there moves the wrong motion, which is why every line is
        // exercised rather than the one that happens to share a name.
        macro_rules! drops_exactly {
            ($pref:ident -> $motion:ident) => {{
                let mut prefs = AnimationSettings::default();
                prefs.$pref = false;
                let m = Motion::from_prefs(&prefs);
                assert!(!m.$motion, "{} must drop its own motion", stringify!($pref));
                let dropped = [m.sidebar_slide, m.canvas_resize, m.zoom, m.scroll_glide]
                    .iter()
                    .filter(|on| !**on)
                    .count();
                assert_eq!(dropped, 1, "{} must drop nothing else", stringify!($pref));
            }};
        }
        drops_exactly!(sidebar_slide -> sidebar_slide);
        drops_exactly!(canvas_resize -> canvas_resize);
        drops_exactly!(zoom -> zoom);
        drops_exactly!(scroll_jumps -> scroll_glide);
    }
}
