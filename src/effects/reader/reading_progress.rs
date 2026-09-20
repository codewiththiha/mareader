//! Debounced progress snapshots. The reader never writes library storage.
use std::time::Duration;
use leptos::prelude::*;
use pdf_engine::types::DocStatus;
use crate::state::AppState;

pub fn reading_progress(state: AppState) {
    let zooming = state.reader.viewer.zooming();
    let timer = StoredValue::new_local(None::<TimeoutHandle>);
    on_cleanup(move || { if let Some(h) = timer.get_value() { h.clear(); } });
    Effect::new(move |_| {
        let status = state.reader.document.status.get();
        let page = state.reader.viewer.page.get();
        let streaming = state.reader.reflow_streaming();
        let _scroll = state.reader.viewer.scroll_top.get();
        let fraction = if streaming { state.reader.stream_fraction() } else { None };
        let num_pages = state.reader.document.num_pages.get();
        if let Some(h) = timer.get_value() { h.clear(); }
        if zooming.get() || status != DocStatus::Ready || page == 0 || page > num_pages { return; }
        let point = crate::runtime::ReadPoint { page, num_pages, fraction };
        timer.set_value(set_timeout_with_handle(move || {
            crate::runtime::reader::progress(state, point);
        }, Duration::from_millis(400)).ok());
    });
}
