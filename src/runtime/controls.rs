//! Value-only chrome projection. Neither side shares reactive owners or DOM handles.
use leptos::prelude::*;
use serde_json::{Value, json};
use crate::state::AppState;

pub fn snapshot(state: AppState) -> Value {
    let r = state.reader;
    json!({
        "title":r.document.title.get(), "author":r.document.author.get(), "numPages":r.document.num_pages.get(),
        "ready":r.document.status.get() == pdf_engine::types::DocStatus::Ready,
        "outline":&*r.document.outline.get(), "page":r.viewer.page.get(),
        "mode":r.viewer.mode.get(), "fit":r.viewer.fit.get(),
        "zoom":r.viewer.zoom.display.get(), "autoScroll":r.viewer.auto_scroll.get(),
        "search":r.search.visible.get(),
    })
}

pub fn apply(state: AppState, value: &Value) {
    let r = state.reader;
    if let Ok(mode) = serde_json::from_value(value["mode"].clone())
        && r.viewer.mode.get_untracked() != mode { r.viewer.mode.set(mode); }
    if let Ok(fit) = serde_json::from_value(value["fit"].clone())
        && r.viewer.fit.get_untracked() != fit { r.viewer.fit.set(fit); }
    if let Some(page) = value["page"].as_u64() {
        let page = (page as u32).clamp(1, r.document.num_pages.get_untracked().max(1));
        if r.viewer.page.get_untracked() != page { r.viewer.page.set(page); }
    }
    if let Some(on) = value["autoScroll"].as_bool()
        && r.viewer.auto_scroll.get_untracked() != on { r.viewer.auto_scroll.set(on); }
    if let Some(on) = value["search"].as_bool()
        && on != r.search.visible.get_untracked() {
            if on { crate::effects::reader::search::resume_search(r); }
            else { crate::effects::reader::search::dismiss_search(r); }
    }
    if let Ok(command) = serde_json::from_value(value["zoomCommand"].clone()) {
        r.viewer.zoom.post(command, true);
    }
}
