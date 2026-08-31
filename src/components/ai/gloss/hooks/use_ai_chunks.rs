//! Chunk ingestion: one window listener (fed by the app-lifetime Tauri
//! bridge in `services::ai`) that turns `pdfreader:ai-chunk` events into
//! state transitions. Listening here — not on Tauri directly — means
//! unmount cleans up a plain window listener and never stacks/drops dead
//! Tauri handlers across document switches.

use leptos::prelude::*;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::types::{AiPhase, GlossPhase};
use crate::services::ai::{AiChunkEvent, AI_CHUNK_EVENT};
use crate::state::AppState;

pub fn use_ai_chunks(state: AppState, ctrl: GlossController) {
    let processing_id = state.reader.gloss.processing_id;

    // The surface is born ON the first chunk (or an error) — never on a
    // timer — so it can never pop open empty.
    let handle = window_event_listener(
        leptos::ev::Custom::new(AI_CHUNK_EVENT),
        move |ev: web_sys::CustomEvent| {
            let Ok(chunk) = serde_wasm_bindgen::from_value::<AiChunkEvent>(ev.detail()) else {
                return;
            };
            match chunk {
                AiChunkEvent::Snapshot(info) => {
                    if let Some(m) = ctrl.open.mark.get_untracked() {
                        ctrl.cache.insert(m.id.clone(), info.clone());
                    }
                    // Land the content — and with it the measure twin's
                    // height — BEFORE the surface is told to expand, the
                    // same order serve_cached uses. Writing it after the
                    // expand leaves one frame where the visible card holds
                    // the full text against a stale small height; the
                    // transient overflow mounts a scrollbar, re-wraps the
                    // text, and the card settles permanently short.
                    ctrl.content.word_info.set(Some(info));
                    processing_id.set(None);
                    if ctrl.content.phase.get_untracked() == AiPhase::Processing {
                        ctrl.content.phase.set(AiPhase::Streaming);
                        if ctrl.geometry.gphase.get_untracked() == GlossPhase::Processing {
                            ctrl.geometry.gphase.set(GlossPhase::Expanded);
                            ctrl.geometry.surface_visible.set(true);
                        }
                    }
                }
                AiChunkEvent::Done => ctrl.content.phase.set(AiPhase::Done),
                AiChunkEvent::Error(err) => {
                    // A failed run must not leave a stale partial snapshot
                    // behind: re-opening this mark has to re-request, not
                    // recall the fragment as if it were the finished answer.
                    if let Some(m) = ctrl.open.mark.get_untracked() {
                        ctrl.cache.remove(&m.id);
                    }
                    ctrl.content.error.set(Some(err));
                    ctrl.content.phase.set(AiPhase::Error);
                    processing_id.set(None);
                    if ctrl.geometry.gphase.get_untracked() == GlossPhase::Processing {
                        ctrl.geometry.gphase.set(GlossPhase::Expanded);
                    }
                    ctrl.geometry.surface_visible.set(true);
                }
            }
        },
    );
    on_cleanup(move || handle.remove());
}
