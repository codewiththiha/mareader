//! Native maximized state for the active route's caption controls. The
//! title bar owns this scoped subscription/probe; a response from a disposed
//! route cannot write into a replacement Library/Reader signal.
//!
//! The trigger is Tauri's own `tauri://resize`, which a route receives because
//! `public/tauri-relay.js` registers the frame's listeners on the main frame —
//! a sub-frame's own event registry is never delivered to (Tauri's docs: an
//! emitted event is scripted into the main frame). Maximizing, restoring and
//! snapping all change the window's size, so that one event covers every way
//! the state changes without going through the caption.

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
    // No `has_tauri()` gate here, and no wait for one. Each runtime page loads
    // `tauri-relay.js` in its own `<head>`, before the module script that boots
    // this app, so the surface's presence is final by the time a bar mounts: a
    // frame with none here is a plain browser and nothing will ever arrive to
    // notify it. Both halves ask on their own and decline quietly —
    // `is_window_maximized()` answers `None`, `tauri_listen` returns without
    // registering — which leaves the glyph on the state this route can defend.
    // Polling for the surface is the worse option: with none it never clears,
    // and while it runs it holds this owner, so the frame cannot report
    // disposal. `Deep CI` caught precisely that — eleven forced frame removals
    // and a virtualizer stage whose look-ahead was never observed.
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
                // `None` is "the window did not say", not "the window is not
                // maximized": the last known answer stays on screen.
                if let Some(answer) = answer {
                    // `try_set` answers `Some(value)` when the signal is gone,
                    // which this loop's next line finds out for itself.
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

// only the changed file was rewritten
