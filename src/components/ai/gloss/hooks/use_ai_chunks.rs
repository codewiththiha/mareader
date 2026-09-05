//! Chunk ingestion: one window listener (fed by the app-lifetime Tauri
//! bridge in `services::ai`) that turns `pdfreader:ai-chunk` events into
//! state transitions. Listening here — not on Tauri directly — means
//! unmount cleans up a plain window listener and never stacks/drops dead
//! Tauri handlers across document switches.
//!
//! Every event carries the id of the run that produced it, and only the run
//! the card is currently waiting on is allowed to write to it — see
//! [`GlossOpen::begin_run`](crate::components::ai::gloss::controller::GlossOpen::begin_run).

use std::sync::Arc;

use leptos::prelude::*;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::phase::{AiPhase, GlossPhase};
use crate::services::ai::{AI_CHUNK_EVENT, AiChunk, AiChunkEvent};
use crate::state::AppState;

pub fn use_ai_chunks(state: AppState, ctrl: GlossController) {
    let processing_id = state.reader.gloss.processing_id;

    // The surface is born ON the first chunk (or an error) — never on a
    // timer — so it can never pop open empty.
    let handle = window_event_listener(
        leptos::ev::Custom::new(AI_CHUNK_EVENT),
        move |ev: web_sys::CustomEvent| {
            let Ok(event) = serde_wasm_bindgen::from_value::<AiChunkEvent>(ev.detail()) else {
                return;
            };
            // Only the run this card is waiting on may write to it. Runs are
            // never cancelled backend-side, so gloss a second word while the
            // first is still thinking and both stream onto this one listener;
            // without the gate the abandoned answer lands on — and is cached
            // under — whichever mark happens to be open when it arrives.
            if !ctrl.open.accepts(&event.run) {
                return;
            }
            match event.chunk {
                AiChunk::Snapshot(info) => {
                    // Bound the answer here, at the door: everything
                    // downstream holds this exact value for the session.
                    let info = Arc::new(info.clamped());
                    if let Some(m) = ctrl.open.mark.get_untracked() {
                        ctrl.cache.insert(m.id, Arc::clone(&info));
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
                AiChunk::Done => {
                    ctrl.content.phase.set(AiPhase::Done);
                    ctrl.open.end_run();
                }
                AiChunk::Error(err) => {
                    ctrl.open.end_run();
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
