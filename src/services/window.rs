//! App-lifetime window-state bridge: the live maximized flag.
//!
//! The frameless caption cluster (app_chrome::window::caption) shows
//! maximize or restore per the window's REAL state, and that state changes
//! under it by more than its own button: Win+Arrow snapping, drag-to-edge,
//! taskbar restores, double-clicking the drag region. Every one of those
//! resizes the window, so one `tauri://resize` subscription re-asks the
//! question for all of them at once and publishes into
//! `UiState::window_maximized`, which is all the cluster ever reads.
//!
//! Installed from the app root (install_app_effects) like the other
//! app-lifetime bridges, and for the same reason a page-scoped subscription
//! would be wrong: the parked closure must never be dropped while Tauri's
//! JS still holds the handler (see services/tauri_listen.rs), and a page
//! owner dies on every route change. Probes are coalesced — one in flight
//! plus one trailing probe — because an interactive resize fires an event
//! per frame, far faster than the round trip completes. The trailing probe
//! ensures a resize during the round trip cannot leave stale state behind.
//!
//! macOS skips the bridge: its windows are not frameless, nothing reads
//! the flag, and there is no reason to spend an IPC per resize there.

use leptos::prelude::*;

use app_chrome::platform::uses_frameless_controls;
use crate::state::AppState;

#[derive(Default)]
struct ProbeState {
    probing: bool,
    pending: bool,
}

impl ProbeState {
    fn request(&mut self) -> bool {
        if self.probing {
            self.pending = true;
            return false;
        }
        self.probing = true;
        true
    }

    fn complete(&mut self) -> bool {
        self.probing = false;
        if !self.pending {
            return false;
        }
        self.pending = false;
        self.probing = true;
        true
    }
}

/// Publish the window's maximized state into `state.ui.window_maximized`:
/// once at install, then on every window resize.
pub fn install_window_state_bridge(state: AppState) {
    if !tauri_bridge::has_tauri() || !uses_frameless_controls() {
        return;
    }

    let probes = StoredValue::new_local(ProbeState::default());
    let probe = move || {
        let mut should_probe = false;
        probes.update_value(|state| should_probe = state.request());
        if !should_probe {
            return;
        }
        let st = state;
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                st.ui.window_maximized
                    .set(app_chrome::window::api::is_window_maximized().await);
                let mut should_probe = false;
                probes.update_value(|state| should_probe = state.complete());
                if !should_probe {
                    break;
                }
            }
        });
    };

    // The install-time answer, so the first caption paint is already right.
    probe();

    crate::services::tauri_listen("tauri://resize", move |_ev| probe());
}

#[cfg(test)]
mod tests {
    use super::ProbeState;

    #[test]
    fn a_resize_during_a_probe_schedules_a_trailing_probe() {
        let mut probes = ProbeState::default();
        assert!(probes.request(), "the first resize starts a probe");
        assert!(
            !probes.request(),
            "a second resize is coalesced while probing"
        );
        assert!(probes.pending);

        assert!(
            probes.complete(),
            "completion immediately starts the pending probe"
        );
        assert!(probes.probing);
        assert!(!probes.pending);
        assert!(!probes.complete());
        assert!(!probes.probing);
    }
}
