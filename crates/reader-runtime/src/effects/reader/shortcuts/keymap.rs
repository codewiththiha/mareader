//! What a navigation key MEANS, as a pure function of the world it
//! landed in.

use reader_core::view::ViewMode;

/// Which way a navigation key points. `-1` is back/up/left, `1` is
/// forward/down/right.
pub(super) type Dir = i32;

/// One thing the reader asked the strip to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NavAction {
    /// Turn to the previous page (paginated modes).
    PagePrev,
    /// Turn to the next page (paginated modes).
    PageNext,
    /// Start the rAF scroll hold: one nudge now, a glide if held.
    HoldLine { dir: Dir, horizontal: bool },
    /// One near-screen step along the strip.
    PageStep { dir: Dir, horizontal: bool },
}

/// The keypress, as everything about it that matters.
#[derive(Debug, Clone, Copy)]
pub(super) struct NavKey<'a> {
    pub key: &'a str,
    pub shift: bool,
    /// The browser's auto-repeat is firing; the hold engine owns a held
    /// key.
    pub repeat: bool,
    pub mode: ViewMode,
    /// The key landed inside a chrome scroller, which owns its arrows.
    pub in_chrome: bool,
    /// The key landed on a button, so Space must activate it instead.
    pub on_button: bool,
}

/// What to do about a keypress: suppress the browser, and which
/// action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NavOutcome {
    pub prevent_default: bool,
    pub action: Option<NavAction>,
}

impl NavOutcome {
    /// The reader claims this key; run `action` if there is one.
    const fn claimed(action: Option<NavAction>) -> Self {
        Self {
            prevent_default: true,
            action,
        }
    }

    /// Not ours: leave the key to the browser.
    const fn passed() -> Self {
        Self {
            prevent_default: false,
            action: None,
        }
    }
}

/// The physical keys that mean an arrow, vim's home row included.
pub(super) fn arrow_key(key: &str) -> Option<&'static str> {
    match key {
        "h" => Some("ArrowLeft"),
        "j" => Some("ArrowDown"),
        "k" => Some("ArrowUp"),
        "l" => Some("ArrowRight"),
        _ => None,
    }
}

pub(super) fn resolve(k: NavKey<'_>) -> NavOutcome {
    // One canonicalisation: every arm below is the arrows' rules.
    let key = arrow_key(k.key).unwrap_or(k.key);
    match key {
        // Left/right: a page turn everywhere except the horizontal strip,
        // where they are the scroll axis.
        "ArrowLeft" | "ArrowRight" => {
            let dir: Dir = if key == "ArrowLeft" { -1 } else { 1 };
            if k.mode == ViewMode::ScrollHorizontal {
                let hold = (!k.in_chrome && !k.repeat).then_some(NavAction::HoldLine {
                    dir,
                    horizontal: true,
                });
                NavOutcome::claimed(hold)
            } else {
                NavOutcome::claimed(Some(page_turn(dir)))
            }
        }
        // Up/down: a page turn in paged modes, a glide down the column.
        "ArrowUp" | "ArrowDown" => {
            let dir: Dir = if key == "ArrowUp" { -1 } else { 1 };
            if k.mode.is_paginated() {
                NavOutcome::claimed(Some(page_turn(dir)))
            } else if k.mode == ViewMode::ScrollVertical && !k.in_chrome {
                let hold = (!k.repeat).then_some(NavAction::HoldLine {
                    dir,
                    horizontal: false,
                });
                NavOutcome::claimed(hold)
            } else {
                NavOutcome::passed()
            }
        }
        "PageUp" | "PageDown" => {
            let dir: Dir = if k.key == "PageUp" { -1 } else { 1 };
            page_step(&k, dir)
        }
        // Space pages the column, Shift+Space back — when not activating.
        " " => {
            if k.on_button {
                return NavOutcome::passed();
            }
            page_step(&k, if k.shift { -1 } else { 1 })
        }
        _ => NavOutcome::passed(),
    }
}

const fn page_turn(dir: Dir) -> NavAction {
    if dir < 0 {
        NavAction::PagePrev
    } else {
        NavAction::PageNext
    }
}

/// A near-screen step, or nothing in the paginated modes and chrome.
fn page_step(k: &NavKey<'_>, dir: Dir) -> NavOutcome {
    if k.in_chrome {
        return NavOutcome::passed();
    }
    match k.mode {
        ViewMode::ScrollVertical => NavOutcome::claimed(Some(NavAction::PageStep {
            dir,
            horizontal: false,
        })),
        ViewMode::ScrollHorizontal => NavOutcome::claimed(Some(NavAction::PageStep {
            dir,
            horizontal: true,
        })),
        _ => NavOutcome::passed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str, mode: ViewMode) -> NavKey<'_> {
        NavKey {
            key: k,
            shift: false,
            repeat: false,
            mode,
            in_chrome: false,
            on_button: false,
        }
    }

    #[test]
    fn arrows_turn_pages_in_the_paginated_modes() {
        for mode in [ViewMode::Single, ViewMode::Spread] {
            assert_eq!(
                resolve(key("ArrowDown", mode)).action,
                Some(NavAction::PageNext)
            );
            assert_eq!(
                resolve(key("ArrowUp", mode)).action,
                Some(NavAction::PagePrev)
            );
            assert_eq!(
                resolve(key("ArrowRight", mode)).action,
                Some(NavAction::PageNext)
            );
        }
    }

    #[test]
    fn arrows_scroll_the_strip_in_the_continuous_modes() {
        assert_eq!(
            resolve(key("ArrowDown", ViewMode::ScrollVertical)).action,
            Some(NavAction::HoldLine {
                dir: 1,
                horizontal: false
            })
        );
        assert_eq!(
            resolve(key("ArrowRight", ViewMode::ScrollHorizontal)).action,
            Some(NavAction::HoldLine {
                dir: 1,
                horizontal: true
            })
        );
        // Left/right still turn pages while the column is the scroll axis.
        assert_eq!(
            resolve(key("ArrowRight", ViewMode::ScrollVertical)).action,
            Some(NavAction::PageNext)
        );
    }

    #[test]
    fn a_repeat_is_claimed_but_does_not_restart_the_hold() {
        let mut k = key("ArrowDown", ViewMode::ScrollVertical);
        k.repeat = true;
        let out = resolve(k);
        assert!(
            out.prevent_default,
            "the browser must not also scroll the page"
        );
        assert_eq!(out.action, None, "the rAF glide is already running");
    }

    #[test]
    fn chrome_scrollers_keep_their_own_arrows() {
        let mut k = key("ArrowDown", ViewMode::ScrollVertical);
        k.in_chrome = true;
        assert_eq!(resolve(k), NavOutcome::passed());

        // The horizontal strip claims the key either way.
        let mut k = key("ArrowRight", ViewMode::ScrollHorizontal);
        k.in_chrome = true;
        let out = resolve(k);
        assert!(out.prevent_default);
        assert_eq!(out.action, None);
    }

    #[test]
    fn space_pages_the_column_and_shift_space_pages_back() {
        assert_eq!(
            resolve(key(" ", ViewMode::ScrollVertical)).action,
            Some(NavAction::PageStep {
                dir: 1,
                horizontal: false
            })
        );
        let mut k = key(" ", ViewMode::ScrollVertical);
        k.shift = true;
        assert_eq!(
            resolve(k).action,
            Some(NavAction::PageStep {
                dir: -1,
                horizontal: false
            })
        );
    }

    #[test]
    fn space_on_a_button_activates_the_button() {
        let mut k = key(" ", ViewMode::ScrollVertical);
        k.on_button = true;
        assert_eq!(
            resolve(k),
            NavOutcome::passed(),
            "a focused button must still be clickable"
        );
    }

    #[test]
    fn page_keys_do_nothing_in_the_paginated_modes() {
        assert_eq!(
            resolve(key("PageDown", ViewMode::Single)),
            NavOutcome::passed()
        );
        assert_eq!(
            resolve(key("PageUp", ViewMode::ScrollVertical)).action,
            Some(NavAction::PageStep {
                dir: -1,
                horizontal: false
            })
        );
    }

    #[test]
    fn an_unmapped_key_is_left_alone() {
        assert_eq!(
            resolve(key("q", ViewMode::ScrollVertical)),
            NavOutcome::passed()
        );
    }

    /// The aliases are the whole point: one row of names, every rule.
    #[test]
    fn the_vim_home_row_is_the_arrows_by_another_name() {
        for mode in [
            ViewMode::Single,
            ViewMode::Spread,
            ViewMode::ScrollVertical,
            ViewMode::ScrollHorizontal,
        ] {
            assert_eq!(resolve(key("j", mode)), resolve(key("ArrowDown", mode)));
            assert_eq!(resolve(key("k", mode)), resolve(key("ArrowUp", mode)));
            assert_eq!(resolve(key("h", mode)), resolve(key("ArrowLeft", mode)));
            assert_eq!(resolve(key("l", mode)), resolve(key("ArrowRight", mode)));
        }
        // Only the bare letters: a capital is another key.
        assert_eq!(
            resolve(key("J", ViewMode::ScrollVertical)),
            NavOutcome::passed()
        );
        let mut k = key("j", ViewMode::ScrollVertical);
        k.in_chrome = true;
        assert_eq!(resolve(k), NavOutcome::passed());
    }
}
