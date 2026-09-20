//! Render only the open PDF's cover; the library never loads PDF bytes.
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use crate::state::AppState;
use crate::services::document::session;

pub(super) fn ensure(state: AppState, path: String, stamp: u64) {
    if state.reader.cover.get_untracked().is_some() { return; }
    spawn_local(async move {
        let Ok(cover) = pdf_engine::api::cover_data_url(&path, 240.0).await else { return; };
        if !session::owns(stamp) { return; }
        state.reader.cover.set(Some(cover.data_url.clone()));
        crate::runtime::emit(serde_json::json!({"type":"cover", "path":path,
            "dataUrl":cover.data_url, "width":cover.width, "height":cover.height}));
    });
}
