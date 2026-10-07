//! Native maximized state for the route's caption controls, probed from
//! Tauri's own `tauri://resize`.

use leptos::prelude::*;

use app_chrome::platform::uses_frameless_controls;

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

/// Publish the native window's actual maximized state:
/// once at install, then on every window resize.
pub(super) fn install(maximized: RwSignal<bool>) {
    // No `has_tauri()` gate: a frame with no API is a plain browser.
    if !uses_frameless_controls() {
        return;
    }

    let probes = StoredValue::new_local(ProbeState::default());
    let probe = move || {
        if probes.try_update_value(ProbeState::request) != Some(true) {
            return;
        }
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                let answer = app_chrome::window::api::is_window_maximized().await;
                // `None` is "the window did not say", not "not maximized".
                if let Some(answer) = answer {
                    // `try_set` answers `Some` when the signal is gone.
                    let _ = maximized.try_set(answer);
                }
                if probes.try_with_value(|_| ()).is_none() {
                    return; // the route that asked is gone; nothing to write to
                }
                if probes.try_update_value(ProbeState::complete) != Some(true) {
                    break;
                }
            }
        });
    };

    // The install-time answer, so the first caption paint is already right.
    probe();

    app_state::tauri_listen::tauri_listen("tauri://resize", move |_ev| probe());
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
