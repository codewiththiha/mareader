//! Chunk ingestion: a window listener turning `mareader:ai-chunk`
//! events into state.

use std::sync::Arc;

use leptos::prelude::*;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::phase::{AiPhase, GlossPhase};
use crate::services::ai::{AI_CHUNK_EVENT, AiChunk, AiChunkEvent};

pub fn use_ai_chunks(state: crate::context::ReaderContext, ctrl: GlossController) {
    let processing_id = state.reader.gloss.processing_id;

    // The surface is born on the first chunk, never on a timer.
    let handle = window_event_listener(
        leptos::ev::Custom::new(AI_CHUNK_EVENT),
        move |ev: web_sys::CustomEvent| {
            let Ok(event) = serde_wasm_bindgen::from_value::<AiChunkEvent>(ev.detail()) else {
                return;
            };
            // Only the awaited run may write; abandoned answers never land.
            if !ctrl.open.accepts(&event.run) {
                return;
            }
            match event.chunk {
                AiChunk::Snapshot(info) => {
                    // Bound the answer here, at the door.
                    let info = Arc::new(info.clamped());
                    if let Some(m) = ctrl.open.mark.get_untracked() {
                        ctrl.cache.insert(m.id, Arc::clone(&info));
                    }
                    // Land the content before the expand, as serve_cached does.
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
                    // Clear the partial snapshot: a reopen must re-request.
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
