//! Page command → scroll: consumes one-shot `page_command` jumps.

use std::rc::Rc;

use leptos::prelude::*;

use reader_core::view::ViewMode;
use virtual_list_leptos::{Align, Virtualizer};

use super::{Arms, JumpGate};

/// Install the `page_command` → scroll arm.
pub(super) fn install(
    arms: Arms,
    vertical: Virtualizer,
    horizontal: Virtualizer,
    gate: Rc<JumpGate>,
) {
    let Arms { state, zooming } = arms;
    let viewer = state.viewer;
    Effect::new(move |_| {
        let Some(zooming) = zooming.try_get() else {
            return;
        };
        let command = viewer.take_page_command();
        let Some(jump) = gate.admit_jump(command, zooming) else {
            return;
        };
        if jump.page == 0 {
            return;
        }
        // Re-assert the display page in case a zoom-held jump replayed after
        // a dominant read.
        if viewer.page.get_untracked() != jump.page {
            viewer.page.set(jump.page);
        }
        // The shell is still placing the strip; do not fight it.
        if viewer.awaiting_anchor.get_untracked() {
            return;
        }
        match viewer.mode.get_untracked() {
            ViewMode::ScrollVertical if state.reflowable() => {
                state
                    .document
                    .content
                    .reflow
                    .scroll_to_page(jump.page, jump.align, jump.mode);
            }
            ViewMode::ScrollVertical => {
                vertical.scroll_to_index((jump.page - 1) as usize, jump.align, jump.mode);
            }
            ViewMode::ScrollHorizontal => {
                let align = if jump.align == Align::Start {
                    Align::Center
                } else {
                    jump.align
                };
                horizontal.scroll_to_index((jump.page - 1) as usize, align, jump.mode);
            }
            ViewMode::Single | ViewMode::Spread => {}
        }
    });
}
