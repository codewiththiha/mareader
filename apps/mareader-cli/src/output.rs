//! JSON serializers for `commands`, `inspect`, `exec`, and `run`.

use mareader_engine::command_spec::SPECS;
use mareader_engine::event::Event;
use mareader_engine::inspect::route_name;
use mareader_engine::notes_panel::page_of;
use mareader_engine::route::Route;
use mareader_engine::session::Session;
use mareader_engine::toast::Tone;
use mareader_workspace::PaneId;
use serde_json::{Value, json};

const ROUTES: &[(Route, &str)] = &[
    (Route::Reader, "reader"),
    (Route::Library, "library"),
    (Route::Dictionary, "dictionary"),
];

/// Serializes every registered `CommandSpec` in `SPECS`.
pub fn commands_json() -> Value {
    let items: Vec<Value> = SPECS
        .iter()
        .map(|spec| {
            let params: Vec<Value> = spec
                .params
                .iter()
                .map(|(name, kind)| json!({ "name": name, "kind": kind }))
                .collect();
            let routes: Vec<&str> = ROUTES
                .iter()
                .filter(|(route, _)| (spec.enabled)(*route))
                .map(|(_, label)| *label)
                .collect();
            json!({
                "id": spec.id,
                "title": spec.title,
                "label": spec.label,
                "menu": spec.menu,
                "shortcut": spec.shortcut,
                "journal": spec.journal,
                "params": params,
                "routes": routes,
            })
        })
        .collect();
    Value::Array(items)
}

/// Serializes the full observable state of `session`.
pub fn inspect_json(session: &Session) -> Value {
    let panes: Vec<Value> = session
        .tree()
        .panes()
        .into_iter()
        .map(|pane| pane_json(session, pane))
        .collect();
    let settings = serde_json::to_value(session.settings()).unwrap_or(Value::Null);
    json!({
        "route": route_name(session.route()),
        "active": session.active().0,
        "panes": panes,
        "library": library_json(session),
        "vocab": {
            "attached": session.vocab().attached(),
            "ready": session.vocab().ready(),
            "words": session.vocab().words(),
        },
        "dict": {
            "attached": session.dict().attached(),
            "languages": session.dict().languages(),
        },
        "settings": settings,
    })
}

/// Serializes every emitted `Event` into structured JSON objects.
pub fn events_json(events: &[Event]) -> Vec<Value> {
    events.iter().map(event_json).collect()
}

/// Extracts `Event::Export` payloads for convenient top-level inspection.
pub fn exports_json(events: &[Event]) -> Vec<Value> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Export {
                name,
                format,
                contents,
            } => Some(json!({
                "name": name,
                "format": format.extension(),
                "contents": contents,
            })),
            _ => None,
        })
        .collect()
}

/// Extracts `Event::WordList` payloads for convenient top-level inspection.
pub fn word_lists_json(events: &[Event]) -> Vec<Value> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::WordList { made, .. } => Some(json!({
                "words": made.words,
                "markdown": &made.markdown,
                "missing": &made.missing,
            })),
            _ => None,
        })
        .collect()
}

fn pane_json(session: &Session, pane: PaneId) -> Value {
    let docs = session.documents();
    let page = docs.current(pane);
    let text = page.and_then(|p| {
        let layer = docs.layer(pane, p)?;
        Some(layer.text.text(0..layer.text.len()))
    });
    let search = docs.place(pane).and_then(|place| {
        let doc = docs.document(place.doc)?;
        let found = session.search().found(place.doc, doc)?;
        let hits: Vec<Value> = found
            .hits
            .iter()
            .map(|h| {
                json!({
                    "page": h.page,
                    "start": h.chars.start,
                    "end": h.chars.end,
                    "snippet": &h.snippet,
                })
            })
            .collect();
        Some(json!({
            "query": &found.query,
            "done": found.done(),
            "more": found.more,
            "indexed": found.indexed,
            "pages": found.pages,
            "hit_count": hits.len(),
            "hits": hits,
        }))
    });
    json!({
        "id": pane.0,
        "title": docs.title(pane),
        "path": docs.path(pane).map(|p| p.display().to_string()),
        "page": page,
        "page_count": docs.page_count(pane),
        "text": text,
        "search": search,
        "marks": marks_json(session, pane),
    })
}

fn marks_json(session: &Session, pane: PaneId) -> Vec<Value> {
    let docs = session.documents();
    let book = session.book_in(pane);
    let title = docs.title(pane).unwrap_or_default();
    let place = docs.place(pane);
    let doc = place.and_then(|p| docs.document(p.doc));
    let (lib, id) = session.study().lib_of(session.library_model(), book, title);
    lib.marks(id)
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id.0.to_string(),
                "kind": format!("{:?}", m.kind),
                "word": &m.word,
                "note": &m.note.value,
                "page": doc.and_then(|d| page_of(d, m.spot)),
            })
        })
        .collect()
}

fn library_json(session: &Session) -> Value {
    let model = session.library_model();
    let books: Vec<Value> = model
        .books()
        .map(|b| {
            json!({
                "id": b.id.0.to_string(),
                "title": &b.title.value,
                "format": b.format,
                "path": b.path().map(|p| p.display().to_string()),
                "missing": b.missing(),
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
        "books": books,
        "shelves": shelves,
        "folders": folders,
    })
}

fn event_json(event: &Event) -> Value {
    match event {
        Event::Toast(t) => json!({
            "kind": "toast",
            "tone": match t.tone {
                Tone::Info => "info",
                Tone::Warning => "warning",
                Tone::Error => "error",
            },
            "message": &t.message,
        }),
        Event::RouteChanged(route) => json!({
            "kind": "route_changed",
            "route": route_name(*route),
        }),
        Event::PaneOpened {
            pane, page, count, ..
        } => json!({
            "kind": "pane_opened",
            "pane": pane.0,
            "page": page,
            "count": count,
        }),
        Event::PaneClosed(pane) => json!({
            "kind": "pane_closed",
            "pane": pane.0,
        }),
        Event::Export {
            name,
            format,
            contents,
        } => json!({
            "kind": "export",
            "name": name,
            "format": format.extension(),
            "contents": contents,
        }),
        Event::WordList { made, .. } => json!({
            "kind": "word_list",
            "words": made.words,
            "markdown": &made.markdown,
            "missing": &made.missing,
        }),
        other => json!({
            "kind": "other",
            "debug": format!("{other:?}"),
        }),
    }
}
