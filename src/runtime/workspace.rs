//! The persistent workspace's protocol endpoint: it mirrors the focused
//! pane's chrome, derives the sidebar (the active panes and the library
//! tree) from value-only payloads, and keeps the persisted library current.
//! The host owns the frames; this realm owns everything the chrome shows.

use std::collections::HashMap;

use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::state::AppState;
use super::emit;

/// One pane in the host's registry, as the sidebar's Active section reads it.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PaneInfo {
    pub pane_id: String,
    pub book_id: String,
    pub title: Option<String>,
    pub format: String,
    pub status: String,
}

/// The signals the workspace views subscribe to. Everything else the host
/// sends lands in the app state or the library; these are the sidebar's own.
#[derive(Clone, Copy)]
pub struct WorkspaceBridge {
    /// The whole pane set the host currently holds, latest first.
    pub panes: RwSignal<Vec<PaneInfo>>,
    /// Bitmaps the host pulled from the active reader, by page.
    pub thumbnails: RwSignal<HashMap<u32, String>>,
    /// Bumped whenever the mirrored document swaps: the thumbnail window
    /// re-keys against the new book's pages.
    pub thumb_epoch: RwSignal<u64>,
    /// The shared Blend paper, painted behind the panes.
    pub blend_paper: RwSignal<Option<String>>,
}

pub fn install(state: AppState) -> WorkspaceBridge {
    let active = RwSignal::new(None::<String>);
    let mirrored = StoredValue::new(json!(null));
    let bridge = WorkspaceBridge {
        panes: RwSignal::new(Vec::new()),
        thumbnails: RwSignal::new(HashMap::new()),
        thumb_epoch: RwSignal::new(0u64),
        blend_paper: RwSignal::new(None::<String>),
    };
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
        match message["type"].as_str() {
            Some("active-state") => {
                let Some(id) = message["paneId"].as_str() else {
                    // The last pane went: the chrome no longer mirrors a
                    // document, so the shell reads as a home again.
                    active.set(None);
                    state.reader.document.status.set(pdf_engine::types::DocStatus::Idle);
                    bridge.thumbnails.update(|thumbs| thumbs.clear());
                    bridge.thumb_epoch.update(|epoch| *epoch += 1);
                    return;
                };
                let Ok(config) =
                    serde_json::from_value::<super::ReaderConfig>(message["config"].clone())
                else {
                    return;
                };
                let v = &message["snapshot"];
                { // Effects flush after this synchronous projection is complete.
                    let was_book = state.reader.document.book_id.get_untracked().clone();
                    let book_id = config.book_id.clone();
                    active.set(Some(id.to_string()));
                    state.settings.set(config.settings);
                    let r = state.reader;
                    r.document.format.set(reader_core::format::format_of(&config.path));
                    r.document.path.set(Some(config.path));
                    r.document.book_id.set(Some(book_id.clone()));
                    r.library_title.set(config.title.clone());
                    r.cover.set(config.cover);
                    r.document.author.set(v["author"].as_str().map(str::to_string));
                    r.document.title.set(v["title"].as_str().map(str::to_string).or(config.title));
                    r.document.status.set(if v["ready"].as_bool().unwrap_or(false) {
                        pdf_engine::types::DocStatus::Ready
                    } else {
                        pdf_engine::types::DocStatus::Opening
                    });
                    r.document.num_pages.set(v["numPages"].as_u64().unwrap_or(1) as u32);
                    r.document.outline.set(std::sync::Arc::new(
                        serde_json::from_value(v["outline"].clone()).unwrap_or_default(),
                    ));
                    super::controls::apply(state, v);
                    if let Some(zoom) = v["zoom"].as_f64() {
                        r.viewer.zoom.display.set(zoom);
                    }
                    // A different book behind the pane: the old bitmaps are
                    // the old document's, and the window must re-key.
                    if was_book.as_deref() != Some(book_id.as_str()) {
                        bridge.thumbnails.update(|thumbs| thumbs.clear());
                        bridge.thumb_epoch.update(|epoch| *epoch += 1);
                    }
                    mirrored.set_value(json!({"settings":state.settings.get_untracked(), "controls":untrack(|| super::controls::snapshot(state))}));
                }
            }
            // The reader moved. Without this the workspace only learns on a
            // focus hop, and the current-page highlight sits on the page the
            // reader left.
            Some("snapshot") => {
                if active.get_untracked().is_some() {
                    apply_snapshot(state, &message["snapshot"]);
                }
            }
            Some("focus") => {
                if let Some(id) = message["paneId"].as_str() {
                    active.set(Some(id.to_string()));
                }
            }
            // The whole registry: the Active section derives from this one
            // payload and the workspace backdrop from its Blend paper.
            Some("panes") => {
                let Some(list) = message["panes"].as_array() else {
                    return;
                };
                let parsed: Vec<PaneInfo> = list
                    .iter()
                    .filter_map(|pane| serde_json::from_value(pane.clone()).ok())
                    .collect();
                bridge.panes.set(parsed);
                if let Some(paper) = message["blendPaper"].as_str() {
                    bridge.blend_paper.set(Some(paper.to_string()));
                } else if message["blendPaper"].is_null() {
                    bridge.blend_paper.set(None);
                }
            }
            // A bitmap the host pulled from the active reader.
            Some("thumbnail") => {
                let (Some(page), Some(url)) =
                    (message["page"].as_u64(), message["dataUrl"].as_str())
                else {
                    return;
                };
                bridge.thumbnails.update(|thumbs| {
                    thumbs.insert(page as u32, url.to_string());
                });
            }
            // The blend source's detected paper, pushed straight through.
            Some("paper-color") => {
                if let Some(color) = message["paper"].as_str() {
                    bridge.blend_paper.set(Some(color.to_string()));
                }
            }
            _ => {}
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
    // The hierarchical tree the host works with: root shelves and the
    // unfiled books, nested — never a flat list, never a pseudo-shelf node.
    Effect::new(move |_| {
        let books = state.library.books.get();
        let shelves = state.library.shelves.get();
        let view = state.library.view.get();
        let roots = crate::components::workspace::tree::library_roots(
            &books,
            &shelves,
            view.sort,
            view.sort_asc,
        );
        emit(json!({"type":"library-tree","roots":roots}));
    });
    emit(json!({"type":"workspace-ready"}));
    bridge
}

/// A live snapshot from the active pane: page, count, metadata and the
/// outline move into the mirrored state, and the page change rides the
/// controls projection (which clamps it against the count it just wrote).
fn apply_snapshot(state: AppState, v: &serde_json::Value) {
    let r = state.reader;
    if let Some(num_pages) = v["numPages"].as_u64() {
        r.document.num_pages.set(num_pages as u32);
    }
    r.document.author.set(v["author"].as_str().map(str::to_string));
    r.document.title.set(v["title"].as_str().map(str::to_string));
    r.document.outline.set(std::sync::Arc::new(
        serde_json::from_value(v["outline"].clone()).unwrap_or_default(),
    ));
    super::controls::apply(state, v);
}
