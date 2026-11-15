//! Read-only document, library, mark, search, and page-render inspection.

mod render;

pub use render::{
    MAX_RENDER_SCALE, MIN_RENDER_SCALE, RenderedPage, encode_base64, events_summary, render_page,
};

use mareader_pdf::active_entry;
use mareader_workspace::PaneId;
use serde_json::{Value, json};

use crate::export::Format;
use crate::notes_panel::page_of;
use crate::route::Route;
use crate::session::Session;

/// Upper bound on pages extracted per [`read_pages`] call.
pub const MAX_READ_PAGES: usize = 20;

/// Resolves an optional numeric pane id, defaulting to the active pane.
pub fn resolve_pane(session: &Session, pane: Option<u64>) -> Result<PaneId, String> {
    let id = pane.map(PaneId).unwrap_or_else(|| session.active());
    if session.tree().panes().contains(&id) {
        Ok(id)
    } else {
        Err(format!("pane {} does not exist", id.0))
    }
}

/// Returns the canonical lowercase route name.
pub fn route_name(route: Route) -> &'static str {
    match route {
        Route::Reader => "reader",
        Route::Library => "library",
        Route::Dictionary => "dictionary",
    }
}

/// Serializes full observable session state (panes, library counts, settings).
pub fn session_state(session: &Session) -> Value {
    let panes: Vec<Value> = session
        .tree()
        .panes()
        .into_iter()
        .map(|pane| pane_summary(session, pane))
        .collect();
    let model = session.library_model();
    let book_count = model.books().count();
    let shelf_count = model.shelves().count();
    let folder_count = model.folders().count();
    json!({
        "route": route_name(session.route()),
        "active_pane": session.active().0,
        "opening": session.documents().opening(),
        "panes": panes,
        "journal_len": session.journal().len(),
        "library": {
            "books": book_count,
            "shelves": shelf_count,
            "folders": folder_count,
        },
        "vocab": {
            "attached": session.vocab().attached(),
            "ready": session.vocab().ready(),
            "words": session.vocab().words(),
        },
        "dict": {
            "attached": session.dict().attached(),
            "languages": session.dict().languages(),
        },
    })
}

/// Reads plain text from `start..=end` (0-based pages) of `pane`'s open book.
pub fn read_pages(
    session: &Session,
    pane: Option<u64>,
    start: Option<usize>,
    end: Option<usize>,
) -> Result<Value, String> {
    let pane_id = resolve_pane(session, pane)?;
    let docs = session.documents();
    let count = docs
        .page_count(pane_id)
        .ok_or_else(|| format!("pane {} has no open document", pane_id.0))?;
    if count == 0 {
        return Ok(json!({
            "pane": pane_id.0,
            "title": docs.title(pane_id),
            "page_count": 0,
            "pages": [],
        }));
    }
    let current = docs.current(pane_id).unwrap_or(0);
    let from = start.unwrap_or(current).min(count - 1);
    let max_to = from.saturating_add(MAX_READ_PAGES - 1).min(count - 1);
    let to = end.unwrap_or(from).max(from).min(max_to);
    let mut pages = Vec::with_capacity(to - from + 1);
    for page in from..=to {
        let text = docs
            .layer(pane_id, page)
            .map(|layer| layer.text.text(0..layer.text.len()))
            .unwrap_or_default();
        let chars = text.chars().count();
        pages.push(json!({
            "page": page,
            "chars": chars,
            "text": text,
        }));
    }
    Ok(json!({
        "pane": pane_id.0,
        "title": docs.title(pane_id),
        "path": docs.path(pane_id).map(|p| p.display().to_string()),
        "current_page": current,
        "page_count": count,
        "start_page": from,
        "end_page": to,
        "pages": pages,
    }))
}

/// Returns the outline / table of contents for `pane`'s open document.
pub fn outline(session: &Session, pane: Option<u64>) -> Result<Value, String> {
    let pane_id = resolve_pane(session, pane)?;
    let docs = session.documents();
    let doc = docs
        .document_of(pane_id)
        .ok_or_else(|| format!("pane {} has no open document", pane_id.0))?;
    let current = docs.current(pane_id).unwrap_or(0);
    let entries = doc.outline();
    let active = active_entry(entries, current);
    let items: Vec<Value> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            json!({
                "index": index,
                "title": &entry.title,
                "page": entry.page,
                "depth": entry.depth,
                "active": active == Some(index),
            })
        })
        .collect();
    Ok(json!({
        "pane": pane_id.0,
        "title": docs.title(pane_id),
        "current_page": current,
        "page_count": doc.page_count(),
        "active_index": active,
        "entries": items,
    }))
}

/// Returns all bookmarks, highlights, and notes on `pane`'s open document.
pub fn marks(session: &Session, pane: Option<u64>) -> Result<Value, String> {
    let pane_id = resolve_pane(session, pane)?;
    let docs = session.documents();
    if docs.place(pane_id).is_none() {
        return Err(format!("pane {} has no open document", pane_id.0));
    }
    let items = pane_marks_vec(session, pane_id);
    Ok(json!({
        "pane": pane_id.0,
        "title": docs.title(pane_id),
        "count": items.len(),
        "marks": items,
    }))
}

/// Returns current search state and matching hits for `pane`'s open document.
pub fn search_hits(session: &Session, pane: Option<u64>, max_hits: usize) -> Result<Value, String> {
    let pane_id = resolve_pane(session, pane)?;
    let docs = session.documents();
    let place = docs
        .place(pane_id)
        .ok_or_else(|| format!("pane {} has no open document", pane_id.0))?;
    let doc = docs
        .document(place.doc)
        .ok_or_else(|| format!("pane {} document is unavailable", pane_id.0))?;
    let Some(found) = session.search().found(place.doc, doc) else {
        return Ok(json!({
            "pane": pane_id.0,
            "query": "",
            "done": true,
            "more": false,
            "indexed": 0,
            "pages": doc.page_count(),
            "hit_count": 0,
            "hits": [],
        }));
    };
    let cap = max_hits.clamp(1, 500);
    let hits: Vec<Value> = found
        .hits
        .iter()
        .take(cap)
        .map(|hit| {
            json!({
                "page": hit.page,
                "start": hit.chars.start,
                "end": hit.chars.end,
                "snippet": &hit.snippet,
            })
        })
        .collect();
    Ok(json!({
        "pane": pane_id.0,
        "query": &found.query,
        "done": found.done(),
        "more": found.more,
        "indexed": found.indexed,
        "pages": found.pages,
        "hit_count": found.hits.len(),
        "truncated": found.hits.len() > cap,
        "hits": hits,
    }))
}

/// Lists library books, shelves, and watched folders with optional filters.
pub fn library(session: &Session, query: Option<&str>, shelf: Option<&str>) -> Value {
    let model = session.library_model();
    let q = query.map(|s| s.trim().to_lowercase());
    let shelf_filter = shelf.map(|s| s.trim().to_lowercase());
    let shelf_ids: Vec<mareader_library::ShelfId> = match shelf_filter.as_deref() {
        Some(f) if !f.is_empty() => model
            .shelves()
            .filter(|s| s.name.value.to_lowercase() == f || s.id.0.to_string() == f)
            .map(|s| s.id)
            .collect(),
        _ => Vec::new(),
    };
    let filter_by_shelf = shelf_filter.as_deref().is_some_and(|s| !s.is_empty());
    let books: Vec<Value> = model
        .books()
        .filter(|b| {
            if filter_by_shelf {
                let member = model.shelves_of(b.id);
                if !shelf_ids.iter().any(|id| member.contains(id)) {
                    return false;
                }
            }
            let Some(q) = q.as_deref().filter(|s| !s.is_empty()) else {
                return true;
            };
            b.title.value.to_lowercase().contains(q)
                || b.path()
                    .is_some_and(|p| p.display().to_string().to_lowercase().contains(q))
        })
        .map(|b| {
            let member: Vec<String> = model
                .shelves_of(b.id)
                .into_iter()
                .map(|id| id.0.to_string())
                .collect();
            json!({
                "id": b.id.0.to_string(),
                "title": &b.title.value,
                "format": b.format,
                "path": b.path().map(|p| p.display().to_string()),
                "missing": b.missing(),
                "shelves": member,
            })
        })
        .collect();
    let shelves: Vec<Value> = model
        .shelves()
        .map(|s| {
            json!({
                "id": s.id.0.to_string(),
                "name": &s.name.value,
                "parent": s.parent.value.map(|p| p.0.to_string()),
            })
        })
        .collect();
    let folders: Vec<Value> = model
        .folders()
        .map(|f| {
            json!({
                "id": f.id.0.to_string(),
                "root": f.root.display().to_string(),
            })
        })
        .collect();
    json!({
        "book_count": books.len(),
        "books": books,
        "shelves": shelves,
        "folders": folders,
    })
}

/// Exports `pane`'s highlights and notes as Markdown, plain text, or JSON.
pub fn export_notes(
    session: &Session,
    pane: Option<u64>,
    format: Option<&str>,
) -> Result<Value, String> {
    let pane_id = resolve_pane(session, pane)?;
    if session.documents().place(pane_id).is_none() {
        return Err(format!("pane {} has no open document", pane_id.0));
    }
    let fmt = match format.unwrap_or("markdown") {
        "markdown" | "md" => Format::Markdown,
        "text" | "txt" => Format::Text,
        "json" => Format::Json,
        other => {
            return Err(format!(
                "unknown export format {other:?} (markdown|text|json)"
            ));
        }
    };
    let (name, contents) = session.export_notes(pane_id, fmt);
    Ok(json!({
        "pane": pane_id.0,
        "name": name,
        "format": fmt.extension(),
        "contents": contents,
    }))
}

fn pane_summary(session: &Session, pane: PaneId) -> Value {
    let docs = session.documents();
    let doc = docs.document_of(pane);
    let mark_count = pane_marks_vec(session, pane).len();
    json!({
        "id": pane.0,
        "title": docs.title(pane),
        "path": docs.path(pane).map(|p| p.display().to_string()),
        "current_page": docs.current(pane),
        "page_count": docs.page_count(pane),
        "reflowable": docs.reflowable(pane),
        "rtl": doc.as_ref().is_some_and(|d| d.right_to_left()),
        "outline_entries": doc.as_ref().map_or(0, |d| d.outline().len()),
        "mark_count": mark_count,
    })
}

fn pane_marks_vec(session: &Session, pane: PaneId) -> Vec<Value> {
    let docs = session.documents();
    let book = session.book_in(pane);
    let title = docs.title(pane).unwrap_or_default();
    let doc = docs.document_of(pane);
    let (lib, id) = session.study().lib_of(session.library_model(), book, title);
    lib.marks(id)
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id.0.to_string(),
                "kind": format!("{:?}", m.kind),
                "word": &m.word,
                "note": &m.note.value,
                "page": doc.as_deref().and_then(|d| page_of(d, m.spot)),
            })
        })
        .collect()
}
