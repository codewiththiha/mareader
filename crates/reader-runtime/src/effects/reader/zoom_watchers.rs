//! The zoom sources: watchers that post commands, never scales.

use std::time::Duration;

use leptos::prelude::*;

use reader_core::zoom_math::FitMode;

use crate::state::ZoomCommand;
use crate::zoom::command::{Gate, posting_gate};
use crate::zoom::config::FOLLOW_SETTLE_MS;
use app_chrome::hooks::use_timeout::use_debounce;
use app_chrome::hooks::use_viewport::use_viewport;
use app_state::SidebarMode;

/// Trailing debounce for a discrete refit, the same quiet window a
/// held follow waits for.
const REFIT_DEBOUNCE: Duration = Duration::from_millis(FOLLOW_SETTLE_MS);

/// Re-resolve the scale when the fit mode, mode or Auto Resize page
/// changes.
pub fn fit_watcher(state: crate::context::ReaderContext) {
    // Built in the caller's owner: one debouncer per watcher, disarmed
    // on cleanup.
    let vs = state.reader.viewer;
    let refit = use_debounce(REFIT_DEBOUNCE, move || {
        // Asked again AT THE FIRE: a gesture owning the transaction is not
        // resolved into.
        let cmd = match posting_gate(vs.zoom.transition.get_untracked()) {
            Gate::StandDown => return,
            // A slide is mid-flight: hand the change to its held commit.
            Gate::Follow => ZoomCommand::Follow,
            Gate::Now if vs.fit.get_untracked() == FitMode::None => ZoomCommand::Constrain,
            Gate::Now => ZoomCommand::Refit,
        };
        // Untweened: a fit must land in the frame it was resolved in.
        vs.zoom.post(cmd, false);
    });

    // The fit the last run answered, so a CHOSEN fit differs from a page
    // change.
    let last_fit = StoredValue::new_local(vs.fit.get_untracked());

    Effect::new(move |_| {
        // Every dependency is a tracked read; the controller re-reads the
        // world when it resolves.
        let fit = vs.fit.get();
        let chosen = last_fit.get_value() != fit;
        let _ = vs.mode.get();
        // The column dial moves what a fit resolves against: a tick re-arms
        // the refit.
        let _ = state.settings.with(|st| st.layout.column_width_pct);
        // Read CONDITIONALLY, so turning Auto Resize off drops the page
        // subscription too.
        if state.settings.with(|st| st.layout.auto_resize) {
            let _ = vs.page.get();
        }
        // Read TRACKED, so a fit click during a gesture lands late, not
        // never.
        if matches!(posting_gate(vs.zoom.transition.get()), Gate::StandDown) {
            return; // a gesture owns the transaction; nothing is consumed
        }
        last_fit.set_value(fit);
        // A fit just picked answers a click: move the page NOW, not a
        // burst.
        if chosen {
            vs.zoom.post(ZoomCommand::Follow, false);
            return;
        }
        // Postpones a pending fire and schedules one at the burst's end.
        refit.trigger();
    });
}

/// Follow the space the page has, frame by frame, keeping the chosen
/// `desired`.
pub fn follow_watcher(state: crate::context::ReaderContext, sidebar: RwSignal<SidebarMode>) {
    let vs = state.reader.viewer;
    // The same end frame, delivered late, for the burst that may skip
    // frames.
    let late = use_debounce(REFIT_DEBOUNCE, move || {
        if matches!(
            posting_gate(vs.zoom.transition.get_untracked()),
            Gate::StandDown
        ) {
            return; // a gesture took the transaction over; it commits itself
        }
        vs.zoom.post(ZoomCommand::Follow, false);
    });
    // The window's box from a `resize` listener; a layout read here
    // would force a flush.
    let viewport = use_viewport();
    // The viewport last measured: a memory, not a dependency.
    let last_viewport = StoredValue::new_local(viewport.get_untracked());

    Effect::new(move |_| {
        let _ = vs.container_size.get();
        let _ = vs.page_margin.get();
        // Tracked so a toggle starts the follow as the rail MOVES.
        let _ = sidebar.get();
        let now = viewport.get_untracked();
        let before = last_viewport.get_value();
        last_viewport.set_value(now);
        // An unchanged viewport means this frame is the sidebar's, whose
        // follow is unconditional.
        let window_dragged = (now.0 - before.0).abs() >= 0.5 || (now.1 - before.1).abs() >= 0.5;
        if window_dragged && !vs.motion.get_untracked().canvas_resize {
            late.trigger();
            return;
        }
        late.cancel();
        // Read UNTRACKED on purpose: after a gesture lands, the next real
        // change answers the ceiling.
        if matches!(
            posting_gate(vs.zoom.transition.get_untracked()),
            Gate::StandDown
        ) {
            return; // a gesture owns the transaction; let it land first
        }
        // One post per frame is not per-frame work: the command slot is
        // single-slot.
        vs.zoom.post(ZoomCommand::Follow, false);
    });
}
