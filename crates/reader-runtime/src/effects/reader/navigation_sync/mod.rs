//! Navigation sync: display `page` vs commanded `page_command`.

mod dominant;
mod jump_gate;
mod page_to_scroll;

use std::rc::Rc;

use leptos::prelude::*;

use reader_core::view::ViewMode;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use crate::state::ReaderState;

use jump_gate::JumpGate;

/// How a commanded jump travels: gliding, or in one step.
pub(crate) fn scroll_mode(state: ReaderState) -> ScrollMode {
    if state.viewer.motion.get_untracked().scroll_glide {
        ScrollMode::Auto
    } else {
        ScrollMode::Instant
    }
}

/// Command an explicit page jump and arm `page_command`.
pub fn request_page(state: ReaderState, page: u32, align: Align) {
    let mode = scroll_mode(state);
    state.viewer.request_page(page, align, mode);
}

/// Command an explicit page jump with a caller-specified [`ScrollMode`].
pub fn request_page_with_mode(state: ReaderState, page: u32, align: Align, mode: ScrollMode) {
    state.viewer.request_page(page, align, mode);
}

/// What every arm needs: state and zoom flag (no echo suppression needed).
#[derive(Clone)]
pub(super) struct Arms {
    pub state: ReaderState,
    /// The tracked form of "a zoom transaction is in flight".
    pub zooming: Signal<bool>,
}

/// Must be called once per pane (its mount), alongside the zoom sources.
pub fn navigation_sync(state: ReaderState, virtualizer: Virtualizer, h_virtualizer: Virtualizer) {
    let arms = Arms {
        state,
        zooming: state.viewer.zooming(),
    };

    // Shared gate: defers dominant reads while a zoom jump replays.
    let gate = Rc::new(JumpGate::default());

    dominant::install(
        arms.clone(),
        ViewMode::ScrollVertical,
        virtualizer.clone(),
        gate.clone(),
    );
    dominant::install(
        arms.clone(),
        ViewMode::ScrollHorizontal,
        h_virtualizer.clone(),
        gate.clone(),
    );
    page_to_scroll::install(arms, virtualizer, h_virtualizer, gate);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::viewer::{PageJump, ViewerSignals};

    #[test]
    fn passive_dominant_page_update_never_arms_page_command() {
        let owner = Owner::new();
        owner.with(|| {
            let viewer = ViewerSignals::default();
            let gate = JumpGate::default();
            assert_eq!(viewer.page.get_untracked(), 1);
            assert_eq!(viewer.page_command.get_untracked(), None);

            // Simulate passive dominant scroll updates.
            for passive_page in [2u32, 3, 15, 42] {
                viewer.page.set(passive_page);
                let cmd = viewer.take_page_command();
                assert_eq!(
                    cmd, None,
                    "passive page {passive_page} must not command scroll"
                );
                assert_eq!(gate.admit_jump(cmd, false), None);
            }
        });
    }

    #[test]
    fn explicit_request_page_arms_one_shot_command_and_replays_after_zoom() {
        let owner = Owner::new();
        owner.with(|| {
            let viewer = ViewerSignals::default();
            let gate = JumpGate::default();

            viewer.request_page(12, Align::Start, ScrollMode::Auto);
            assert_eq!(viewer.page.get_untracked(), 12);
            let cmd = viewer.take_page_command();
            assert_eq!(
                cmd,
                Some(PageJump {
                    page: 12,
                    align: Align::Start,
                    mode: ScrollMode::Auto,
                })
            );
            // Held while zooming, then replayed once zoom settles.
            assert_eq!(gate.admit_jump(cmd, true), None);
            assert_eq!(gate.pending(), Some(12));
            assert_eq!(
                gate.admit_jump(None, false),
                Some(PageJump {
                    page: 12,
                    align: Align::Start,
                    mode: ScrollMode::Auto,
                })
            );
            assert_eq!(gate.pending(), None);
        });
    }
}
