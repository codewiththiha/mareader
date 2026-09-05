//! What a navigation key MEANS, as a pure function.
//!
//! The dispatch used to be a chain of `match ev.key()` arms with the view
//! mode, the chrome-scroller check, the key-repeat flag and the Shift state
//! tested inline against `web_sys` types. Every one of those decisions is
//! interesting — Space pages the column but must still click a focused
//! button; arrows turn pages in the paginated modes and scroll in the
//! continuous ones; a chrome scroller owns its own arrows — and none of them
//! were reachable from a test without a browser and a synthesised event.
//!
//! So the decision is separated from the doing. [`resolve`] takes a plain
//! description of the keypress and the world it landed in and answers with an
//! outcome; `navigation.rs` reads the event, calls this, and performs it.
//! Should the reader ever get a custom keymap, this is the table it edits.

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
    /// Start the rAF scroll hold: one nudge now, a continuous glide if the
    /// key stays down.
    HoldLine { dir: Dir, horizontal: bool },
    /// One near-screen step along the strip.
    PageStep { dir: Dir, horizontal: bool },
}

/// The keypress, as everything about it that matters.
#[derive(Debug, Clone, Copy)]
pub(super) struct NavKey<'a> {
    pub key: &'a str,
    pub shift: bool,
    /// The browser's auto-repeat is firing. The hold engine, not the browser,
    /// owns a held key, so a repeat must not restart it.
    pub repeat: bool,
    pub mode: ViewMode,
    /// The key landed inside a chrome scroller (thumbnails, outline, a
    /// popover). Those own their own arrow keys; the reader must not steal
    /// them.
    pub in_chrome: bool,
    /// The key landed on a button. Space has to activate it rather than page
    /// the document out from under it.
    pub on_button: bool,
}

/// What to do about a keypress: whether the browser's own handling has to be
/// suppressed, and which action (if any) to run.
///
/// The two are genuinely independent. An arrow inside a horizontal strip is
/// claimed by the reader even when nothing comes of it — letting the browser
/// scroll the page as well would move the strip twice — while a repeat of the
/// same key is claimed and then deliberately dropped, because the hold engine
/// is already gliding.
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

    /// Not ours: leave the key to the browser (or to whatever chrome the
    /// focus is in).
    const fn passed() -> Self {
        Self {
            prevent_default: false,
            action: None,
        }
    }
}

/// The keymap.
pub(super) fn resolve(k: NavKey<'_>) -> NavOutcome {
    match k.key {
        // Left/right: a page turn everywhere except the horizontal strip,
        // where they are the scroll axis.
        "ArrowLeft" | "ArrowRight" => {
            let dir: Dir = if k.key == "ArrowLeft" { -1 } else { 1 };
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
        // Up/down: a page turn in the paginated modes, a reading nudge (and
        // then a glide) down the column in the continuous one.
        "ArrowUp" | "ArrowDown" => {
            let dir: Dir = if k.key == "ArrowUp" { -1 } else { 1 };
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
        // Space pages the column, Shift+Space pages back — but only when it is
        // not activating something.
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

/// A near-screen step along whichever strip is scrollable, or nothing at all
/// in the paginated modes and inside chrome.
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

        // The horizontal strip is the exception: the key is claimed either
        // way, because letting the browser scroll it too would move it twice.
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
        assert_eq!(resolve(key("q", ViewMode::ScrollVertical)), NavOutcome::passed());
    }
}
