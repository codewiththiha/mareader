//! Window chrome mirrors only the focused pane. Host owns the tree and all frames.
use leptos::prelude::*;
use serde_json::json;
use crate::state::AppState;
use super::emit;

pub fn install(state: AppState) {
    let active = RwSignal::new(None::<String>);
    let mirrored = StoredValue::new(json!(null));
    super::library::install(state);
    super::listen(move |message| {
        if message["type"] == "flush-chrome" {
            crate::effects::appearance::flush_appearance_commit();
            if let Some(pane_id) = active.get_untracked() {
                emit(json!({"type":"chrome-change","paneId":pane_id,
                    "settings":state.settings.get_untracked(),"controls":untrack(|| super::controls::snapshot(state))}));
            }
            return;
        }
        if message["type"] != "active-state" { return; }
        let Some(id) = message["paneId"].as_str() else { active.set(None); return; };
        let Ok(config) = serde_json::from_value::<super::ReaderConfig>(message["config"].clone()) else { return; };
        let v = &message["snapshot"];
        { // Effects flush after this synchronous projection is complete.
            active.set(Some(id.to_string()));
            state.settings.set(config.settings);
            let r = state.reader;
            r.document.format.set(reader_core::format::format_of(&config.path));
            r.document.path.set(Some(config.path));
            r.document.book_id.set(Some(config.book_id));
            r.library_title.set(config.title.clone());
            r.cover.set(config.cover);
            r.document.author.set(v["author"].as_str().map(str::to_string));
            r.document.title.set(v["title"].as_str().map(str::to_string).or(config.title));
            r.document.status.set(if v["ready"].as_bool().unwrap_or(false) { pdf_engine::types::DocStatus::Ready } else { pdf_engine::types::DocStatus::Opening });
            r.document.num_pages.set(v["numPages"].as_u64().unwrap_or(1) as u32);
            r.document.outline.set(std::sync::Arc::new(serde_json::from_value(v["outline"].clone()).unwrap_or_default()));
            super::controls::apply(state, v);
            if let Some(zoom) = v["zoom"].as_f64() { r.viewer.zoom.display.set(zoom); }
            mirrored.set_value(json!({"settings":state.settings.get_untracked(), "controls":untrack(|| super::controls::snapshot(state))}));
        }
    });
    Effect::new(move |_| {
        let settings = state.settings.get();
        let controls = super::controls::snapshot(state);
        let Some(pane_id) = active.get() else { return; };
        let value = json!({"settings":settings,"controls":controls});
        if mirrored.get_value() != value {
            mirrored.set_value(value);
            emit(json!({"type":"chrome-change","paneId":pane_id,"settings":settings,"controls":controls}));
        }
    });
    Effect::new(move |_| {
        if let Some((command, _, _)) = state.reader.viewer.zoom.commands.get()
            && let Some(pane_id) = active.get_untracked() {
                emit(json!({"type":"pane-command","paneId":pane_id,"command":{"type":"controls","zoomCommand":command}}));
        }
    });
    Effect::new(move |_| {
        let books = state.library.books.get();
        let shelves = state.library.shelves.get();
        let rows: Vec<_> = library_core::book::book_rows(&books).map(|b| json!({
            "id":b.id,"title":b.title(),"path":b.path(),"missing":b.missing,
            "format":match b.format { reader_core::format::Format::Pdf=>"pdf", reader_core::format::Format::Text=>"txt", reader_core::format::Format::Markdown=>"md" }
        })).collect();
        emit(json!({"type":"library-tree","books":rows,"shelves":shelves}));
    });
    emit(json!({"type":"workspace-ready"}));
}
