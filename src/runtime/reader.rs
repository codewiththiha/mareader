//! A single reader's protocol endpoint. No library state is imported here.
use leptos::prelude::*;
use serde_json::json;
use crate::state::AppState;
use super::{ReaderConfig, ReadPoint, emit};

pub fn install(state: AppState, format: &'static str) {
    let opened = RwSignal::new(false);
    let chrome = expect_context::<super::ChromeVisibility>();
    let closing = RwSignal::new(false);
    super::listen(move |message| {
        match message["type"].as_str() {
            Some("open") if !opened.get_untracked() && !closing.get_untracked() => {
                match serde_json::from_value::<ReaderConfig>(message["config"].clone()) {
                    Ok(config) if config.format == format => {
                        opened.set(true);
                        state.settings.set(config.settings);
                        state.reader.library_title.set(config.title);
                        state.reader.cover.set(config.cover);
                        state.reader.document.book_id.set(Some(config.book_id));
                        crate::services::document::open::open_selected(
                            state, config.path, config.resume_page, config.resume_fraction,
                        );
                    }
                    _ => emit(json!({"type":"error", "message":"Invalid reader configuration"})),
                }
            }
            Some("set-settings") if !closing.get_untracked() => {
                if let Ok(mut settings) = serde_json::from_value(message["settings"].clone()) {
                    reader_core::settings::sanitize(&mut settings);
                    pdf_engine::api::set_scrub_mode(false);
                    state.settings.set(settings);
                }
            }
            // The workspace's Blend state, broadcast to every pane: the shared
            // paper paints now, not on the next scroll.
            Some("set-blend") if !closing.get_untracked() => {
                crate::effects::reader::blend_epoch::apply(state, message["paper"].as_str());
            }
            Some("chrome-state") => {
                chrome.bar.set(message["bar"].as_bool().unwrap_or(false));
                chrome.rail.set(message["rail"].as_bool().unwrap_or(false));
            }
            Some("controls") if !closing.get_untracked() => super::controls::apply(state, &message),
            Some("appearance-preview") if !closing.get_untracked() => {
                if let Ok(a) = serde_json::from_value(message["appearance"].clone()) {
                    pdf_engine::api::set_scrub_mode(true);
                    crate::effects::app::theme::paint_appearance_now(a, message["ink"].as_f64().unwrap_or(100.0));
                }
            }
            Some("dispose") if !closing.get_untracked() => {
                closing.set(true);
                // Invalidate opens and flush BEFORE releasing the owner. The
                // async engine destruction is awaited before DISPOSED.
                crate::services::document::close::dispose_document(state);
            }
            _ => {}
        }
    });
    // Persistent settings cross the bridge as values. Session signals never do.
    Effect::new(move |_| {
        let settings = state.settings.get();
        if opened.get() && !closing.get() {
            emit(json!({"type":"settings", "settings":settings}));
        }
    });
    Effect::new(move |_| {
        let snapshot = super::controls::snapshot(state);
        if opened.get() && !closing.get() { emit(json!({"type":"snapshot","snapshot":snapshot})); }
    });
    emit(json!({"type":"ready", "format":format}));
}

pub fn progress(state: AppState, point: ReadPoint) {
    emit(json!({
        "type":"progress",
        "bookId":state.reader.document.book_id.get_untracked(),
        "path":state.reader.document.path.get_untracked(),
        "page":point.page, "numPages":point.num_pages, "fraction":point.fraction,
    }));
}

pub fn record(state: AppState, path: &str, title: Option<String>, point: ReadPoint) {
    emit(json!({
        "type":"metadata", "bookId":state.reader.document.book_id.get_untracked(),
        "path":path, "title":title, "author":state.reader.document.author.get_untracked(),
        "page":point.page, "numPages":point.num_pages, "fraction":point.fraction,
    }));
}
