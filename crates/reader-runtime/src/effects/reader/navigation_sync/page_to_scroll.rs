//! Page → scroll: a page write commands the strip, one arm per axis.

use std::rc::Rc;

use leptos::prelude::*;

use reader_core::view::ViewMode;
use virtual_list_leptos::{Align, Virtualizer};

use super::{Arms, JumpGate, scroll_mode};

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
        // A page write means a page-cut strip only for PDFs here.
        if axis == ViewMode::ScrollVertical && state.reflowable() {
            return;
        }
        // The gate holds the write mid-zoom and replays it after.
        let Some(page_now) = page.try_get() else {
            return;
        };
        let Some(zooming) = zooming.try_get() else {
            return;
        };
        let Some((target, reassert)) = gate.admit(page_now, zooming) else {
            // A stand-down consumed the run; drop the echo flag.
            if zooming {
                suppress.set(false);
            }
            return;
        };
        // A replay re-asserts the page it jumped to.
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
        // The shell is still placing the strip; do not fight it.
        if state.viewer.awaiting_anchor.get_untracked() {
            return;
        }
        // The glide is the animation; the jump is not.
        v.scroll_to_index((target - 1) as usize, align, scroll_mode(state));
    });
}
