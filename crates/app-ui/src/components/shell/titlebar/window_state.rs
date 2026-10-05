//! Native maximized state for the active route's caption controls. The
//! title bar owns this scoped subscription/probe; a response from a disposed
//! route cannot write into a replacement Library/Reader signal.

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
    if !tauri_bridge::has_tauri() || !uses_frameless_controls() {
        return;
    }

    let probes = StoredValue::new_local(ProbeState::default());
    let probe = move || {
        if probes.try_update_value(ProbeState::request) != Some(true) {
            return;
        }
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                let value = app_chrome::window::api::is_window_maximized().await;
                if probes.try_with_value(|_| ()).is_none() || maximized.try_set(value).is_some() {
                    return;
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
