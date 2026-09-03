//! Page → scroll: a page write commands the strip.
//!
//! One arm per axis; they differ only in the view mode they answer for and in
//! where the page lands (the column starts pages at the top, the horizontal
//! strip centres them). Everything interesting is in [`JumpGate`], which
//! decides whether a given run is a navigation at all.

use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use crate::state::ReaderState;

use super::JumpGate;
use super::Arms;

/// How a page-to-scroll jump should travel: gliding while the reader's scroll
/// switch allows it, in one step when it does not. Read UNTRACKED, because the
/// flag that stops a jump gliding must not be what re-runs the jump.
fn scroll_mode(state: ReaderState) -> ScrollMode {
    if state.viewer.motion.get_untracked().scroll_glide {
        ScrollMode::Auto
    } else {
        ScrollMode::Instant
    }
}

/// Install the page → scroll arm for one axis.
pub(super) fn install(
    arms: Arms,
    axis: ViewMode,
    v: Virtualizer,
    gate: Rc<JumpGate>,
    align: Align,
) {
    let Arms {
        state,
        suppress,
        zooming,
        ..
    } = arms;
    let page = state.viewer.page;
    let mode = state.viewer.mode;
    Effect::new(move |_| {
        if mode.get() != axis {
            return;
        }
        // Scroll restoration is the transaction's job while one is open;
        // letting a page write fight the anchor mid-zoom is the other half
        // of the loop. The gate holds the write instead of losing it, and
        // the tracked `zooming` read is what brings this effect back on
        // the frame the transaction closes, to replay it.
        let page_now = page.get();
        let zooming = zooming.get();
        let Some((target, reassert)) = gate.admit(page_now, zooming) else {
            // A stand-down consumed the run. The echo flag's scroll event
            // is never coming (both arms and the DOM echo stand down for
            // the transaction's duration), so it must not survive to eat
            // the replay either.
            if zooming {
                suppress.set(false);
            }
            return;
        };
        // A replay against a clobbered page signal re-asserts the page:
        // the counter must lead the strip, or the next scroll event would
        // "correct" the strip right back off the jumped-to page.
        if reassert {
            suppress.set(false);
            page.set(target);
        }
        if suppress.get() {
            suppress.set(false);
            return;
        }
        if target == 0 {
            return;
        }
        // A strip that is still being placed on `viewer.page` by its shell
        // (`ScrollShell::anchor_to_page`) will read the page itself, on a
        // bound container and instantly; a glide commanded here on top of
        // that would only fight it. UNTRACKED: the anchor landing is not a
        // navigation, so it must not replay this arm.
        if state.viewer.awaiting_anchor.get_untracked() {
            return;
        }
        // The glide is the animation; the jump is not. With the reader's
        // scroll switch off the column lands on the page in one write
        // (`Auto` is what resolves to a smooth scroll when the distance is
        // short, so this is the only decision to make here).
        v.scroll_to_index((target - 1) as usize, align, scroll_mode(state));
    });
}
