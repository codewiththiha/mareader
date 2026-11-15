//! Headless and attached live-UI execution backends for the MCP server.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mareader_engine::command_spec::SPECS;
use mareader_engine::inspect;
use mareader_engine::journal::Step;
use serde_json::{Map, Value, json};

use crate::client::ControlClient;
use crate::headless::HeadlessRuntime;
use crate::image_io::emit_rendered_page;
use crate::jail::RootJail;
use crate::ui_catalog::UI_COMMANDS;

const LIVE_POLL_LIMIT: Duration = Duration::from_secs(15);
const LIVE_POLL_STEP: Duration = Duration::from_millis(25);

/// Execution target for the MCP server: headless or live desktop UI.
pub enum Backend {
    Headless {
        rt: HeadlessRuntime,
        jail: RootJail,
    },
    Live {
        client: ControlClient,
        jail: RootJail,
    },
}

impl Backend {
    /// Creates an in-process headless runtime with optional root confinement.
    pub fn headless(state_dir: Option<PathBuf>, root: Option<PathBuf>) -> Result<Self, String> {
        let jail = RootJail::new(root)?;
        let rt = HeadlessRuntime::open(state_dir);
        Ok(Self::Headless { rt, jail })
    }

    /// Attaches to a live desktop Mareader window over loopback TCP.
    pub fn live(client: ControlClient, root: Option<PathBuf>) -> Result<Self, String> {
        let jail = RootJail::new(root)?;
        Ok(Self::Live { client, jail })
    }

    /// Whether this backend is attached to a live UI window.
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Live { .. })
    }

    /// Lists engine and UI commands, optionally filtered by kind or query text.
    pub fn list_commands(&self, kind: Option<&str>, query: Option<&str>) -> Result<Value, String> {
        let q = query.map(|s| s.trim().to_lowercase());
        let include_engine = !matches!(kind, Some("ui"));
        let include_ui = !matches!(kind, Some("engine"));

        let engine: Vec<Value> = if include_engine {
            SPECS
                .iter()
                .filter(|spec| matches_filter(q.as_deref(), spec.id, spec.title, spec.label))
                .map(|spec| {
                    let params: Vec<Value> = spec
                        .params
                        .iter()
                        .map(|(k, v)| json!({ "name": k, "kind": v }))
                        .collect();
                    let can_run = match self {
                        Self::Headless { rt, .. } => Some((spec.can_run)(&rt.session).is_ok()),
                        Self::Live { .. } => None,
                    };
                    json!({
                        "id": spec.id,
                        "title": spec.title,
                        "label": spec.label,
                        "menu": spec.menu,
                        "shortcut": spec.shortcut,
                        "journal": spec.journal,
                        "params": params,
                        "can_run": can_run,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        let ui: Vec<Value> = if include_ui {
            UI_COMMANDS
                .iter()
                .filter(|meta| matches_filter(q.as_deref(), meta.id, meta.title, meta.label))
                .map(|meta| {
                    json!({
                        "id": meta.id,
                        "title": meta.title,
                        "label": meta.label,
                        "menu": meta.menu,
                        "shortcut": meta.shortcut,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        Ok(json!({ "engine": engine, "ui": ui }))
    }

    /// Returns current route, panes, documents, library counts, and UI state.
    pub fn get_state(&self) -> Result<Value, String> {
        match self {
            Self::Live { client, .. } => client.state(),
            Self::Headless { rt, .. } => Ok(headless_state(&rt.session)),
        }
    }

    /// Opens a document file, waits until ready, and optionally jumps.
    pub fn open_document(
        &mut self,
        path: &str,
        pane: Option<u64>,
        page: Option<usize>,
    ) -> Result<Value, String> {
        self.jail().check_read(Path::new(path))?;
        let mut params = Map::new();
        params.insert("path".to_owned(), Value::String(path.to_owned()));
        if let Some(p) = pane {
            params.insert("pane".to_owned(), json!(p));
        }
        let open_res = self.run_command("reader.open", Value::Object(params), None)?;
        if let Self::Live { client, .. } = self {
            wait_live_open(client)?;
        }
        if let Some(target_page) = page {
            let mut go = Map::new();
            go.insert("page".to_owned(), json!(target_page));
            if let Some(p) = pane {
                go.insert("pane".to_owned(), json!(p));
            }
            let _ = self.run_command("page.go", Value::Object(go), None)?;
        }
        let state = self.get_state()?;
        Ok(json!({
            "opened": path,
            "result": open_res,
            "state": state,
        }))
    }

    /// Reads real text from `start..=end` (0-based pages) of the open document.
    pub fn read_pages(
        &self,
        pane: Option<u64>,
        start: Option<usize>,
        end: Option<usize>,
    ) -> Result<Value, String> {
        match self {
            Self::Headless { rt, .. } => inspect::read_pages(&rt.session, pane, start, end),
            Self::Live { client, .. } => client.query(
                "read_pages",
                json!({ "pane": pane, "start_page": start, "end_page": end }),
            ),
        }
    }

    /// Returns the active document's outline / table of contents.
    pub fn get_outline(&self, pane: Option<u64>) -> Result<Value, String> {
        match self {
            Self::Headless { rt, .. } => inspect::outline(&rt.session, pane),
            Self::Live { client, .. } => client.query("outline", json!({ "pane": pane })),
        }
    }

    /// Searches the open document and waits for background indexing to finish.
    pub fn search_document(
        &mut self,
        query: &str,
        pane: Option<u64>,
        max_hits: usize,
    ) -> Result<Value, String> {
        let mut params = Map::new();
        params.insert("query".to_owned(), Value::String(query.to_owned()));
        if let Some(p) = pane {
            params.insert("pane".to_owned(), json!(p));
        }
        let _ = self.run_command("page.search", Value::Object(params), None)?;
        match self {
            Self::Headless { rt, .. } => inspect::search_hits(&rt.session, pane, max_hits),
            Self::Live { client, .. } => wait_live_search(client, pane, max_hits),
        }
    }

    /// Lists bookmarks, highlights, and notes on the open document.
    pub fn list_marks(&self, pane: Option<u64>) -> Result<Value, String> {
        match self {
            Self::Headless { rt, .. } => inspect::marks(&rt.session, pane),
            Self::Live { client, .. } => client.query("marks", json!({ "pane": pane })),
        }
    }

    /// Selects `text` on `page`, highlights it, and optionally attaches `note`.
    pub fn annotate_text(
        &mut self,
        page: usize,
        text: &str,
        pen: Option<&str>,
        note: Option<&str>,
        pane: Option<u64>,
    ) -> Result<Value, String> {
        let mut sel = Map::new();
        sel.insert("page".to_owned(), json!(page));
        sel.insert("text".to_owned(), Value::String(text.to_owned()));
        if let Some(p) = pane {
            sel.insert("pane".to_owned(), json!(p));
        }
        let _ = self.run_command("mark.select", Value::Object(sel), None)?;

        let mut hi = Map::new();
        hi.insert(
            "pen".to_owned(),
            Value::String(pen.unwrap_or("full").to_owned()),
        );
        if let Some(p) = pane {
            hi.insert("pane".to_owned(), json!(p));
        }
        let _ = self.run_command("mark.highlight", Value::Object(hi), None)?;

        if let Some(body) = note.filter(|s| !s.trim().is_empty()) {
            let note_params = json!({ "action": "set_note", "text": body });
            let _ = self.run_command("study.act", note_params, None)?;
        }
        self.list_marks(pane)
    }

    /// Exports highlights and notes as Markdown, plain text, or JSON.
    pub fn export_notes(&self, format: Option<&str>, pane: Option<u64>) -> Result<Value, String> {
        match self {
            Self::Headless { rt, .. } => inspect::export_notes(&rt.session, pane, format),
            Self::Live { client, .. } => {
                let params = json!({ "pane": pane, "format": format });
                client.query("export_notes", params)
            }
        }
    }

    /// Lists books, shelves, and folders in the Mareader library.
    pub fn list_library(&self, query: Option<&str>, shelf: Option<&str>) -> Result<Value, String> {
        match self {
            Self::Headless { rt, .. } => Ok(inspect::library(&rt.session, query, shelf)),
            Self::Live { client, .. } => {
                let params = json!({ "query": query, "shelf": shelf });
                client.query("library", params)
            }
        }
    }

    /// Headlessly rasterizes a document page to PNG (file or inline base64).
    pub fn render_page(
        &self,
        pane: Option<u64>,
        page: Option<usize>,
        scale: f32,
        path: Option<&str>,
    ) -> Result<Value, String> {
        if let Some(target) = path {
            self.jail().check_write(Path::new(target))?;
        }
        match self {
            Self::Headless { rt, .. } => {
                let rendered = inspect::render_page(&rt.session, pane, page, scale)?;
                emit_rendered_page(rendered, path.map(Path::new))
            }
            Self::Live { client, .. } => client.query(
                "render_page",
                json!({ "pane": pane, "page": page, "scale": scale, "path": path }),
            ),
        }
    }

    /// Executes one command by id with JSON params and optional coalesce key.
    pub fn run_command(
        &mut self,
        id: &str,
        mut params: Value,
        coalesce: Option<String>,
    ) -> Result<Value, String> {
        self.jail().check_command_paths(id, &params)?;
        if let Some(key) = coalesce {
            let mut map = match params {
                Value::Object(m) => m,
                Value::Null => Map::new(),
                _ => return Err("params must be a JSON object".to_owned()),
            };
            map.insert("coalesce".to_owned(), Value::String(key));
            params = Value::Object(map);
        }
        match self {
            Self::Live { client, .. } => client.exec(id, params),
            Self::Headless { rt, .. } => {
                if id.starts_with("ui.") {
                    return Err(format!(
                        "UI command {id:?} requires a live window (--control-file)"
                    ));
                }
                rt.clock += 0.05;
                let done = rt.session.execute_json(id, params, rt.clock)?;
                let events = rt.settle_jobs();
                rt.persist();
                let clipboard = rt.host.take_clipboard();
                Ok(json!({
                    "id": id,
                    "dirty": done.dirty,
                    "events": inspect::events_summary(&events),
                    "clipboard": clipboard,
                    "state": headless_state(&rt.session),
                }))
            }
        }
    }

    /// Executes a batch of `{id, params}` steps in order.
    pub fn run_batch(&mut self, steps: Vec<Step>) -> Result<Value, String> {
        for step in &steps {
            self.jail().check_command_paths(&step.id, &step.params)?;
        }
        let payload = json!({ "commands": steps });
        self.run_command("command.batch", payload, None)
    }

    /// Returns recorded journal entries and optionally clears the journal.
    pub fn journal(&mut self, clear: bool) -> Result<Value, String> {
        match self {
            Self::Live { client, .. } => client.journal(clear),
            Self::Headless { rt, .. } => {
                let entries = rt.session.journal().entries();
                if clear {
                    rt.session.clear_journal();
                }
                Ok(json!({ "entries": entries }))
            }
        }
    }

    /// Replays a list of journal steps without re-recording them.
    pub fn replay(&mut self, steps: Vec<Step>) -> Result<Value, String> {
        for step in &steps {
            self.jail().check_command_paths(&step.id, &step.params)?;
        }
        match self {
            Self::Live { client, .. } => client.replay(&steps),
            Self::Headless { rt, .. } => {
                rt.clock += 0.05;
                let count = rt.session.replay(&steps, rt.clock)?;
                let events = rt.settle_jobs();
                rt.persist();
                Ok(json!({
                    "replayed": count,
                    "events": inspect::events_summary(&events),
                    "state": headless_state(&rt.session),
                }))
            }
        }
    }

    /// Injects synthetic pointer, scroll, key, or text input in live UI mode.
    pub fn ui_input(&self, actions: Value) -> Result<Value, String> {
        match self {
            Self::Live { client, .. } => client.input(actions),
            Self::Headless { .. } => {
                Err("ui_input requires a live window (--control-file)".to_owned())
            }
        }
    }

    /// Captures a live PNG screenshot, confined to `--root` when set.
    pub fn ui_screenshot(&self, path: Option<&str>) -> Result<Value, String> {
        if let Some(target) = path {
            self.jail().check_write(Path::new(target))?;
        }
        match self {
            Self::Live { client, .. } => client.screenshot(path),
            Self::Headless { .. } => {
                Err("ui_screenshot requires a live window (--control-file)".to_owned())
            }
        }
    }

    fn jail(&self) -> &RootJail {
        match self {
            Self::Headless { jail, .. } | Self::Live { jail, .. } => jail,
        }
    }
}

fn matches_filter(q: Option<&str>, id: &str, title: &str, label: &str) -> bool {
    let Some(q) = q.filter(|s| !s.is_empty()) else {
        return true;
    };
    id.to_lowercase().contains(q)
        || title.to_lowercase().contains(q)
        || label.to_lowercase().contains(q)
}

fn headless_state(session: &mareader_engine::session::Session) -> Value {
    let mut state = inspect::session_state(session);
    if let Some(obj) = state.as_object_mut() {
        obj.insert("mode".to_owned(), json!("headless"));
    }
    state
}

fn wait_live_open(client: &ControlClient) -> Result<(), String> {
    let deadline = Instant::now() + LIVE_POLL_LIMIT;
    while Instant::now() < deadline {
        let state = client.state()?;
        if !state
            .get("opening")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(());
        }
        std::thread::sleep(LIVE_POLL_STEP);
    }
    Ok(())
}

fn wait_live_search(
    client: &ControlClient,
    pane: Option<u64>,
    max_hits: usize,
) -> Result<Value, String> {
    let deadline = Instant::now() + LIVE_POLL_LIMIT;
    loop {
        let res = client.query("search", json!({ "pane": pane, "max_hits": max_hits }))?;
        if res.get("done").and_then(Value::as_bool).unwrap_or(true) || Instant::now() >= deadline {
            return Ok(res);
        }
        std::thread::sleep(LIVE_POLL_STEP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_confinement_rejects_paths_outside_root() {
        let Ok(jail) = tempfile::tempdir() else {
            return;
        };
        let Ok(outside) = tempfile::tempdir() else {
            return;
        };
        let outside_file = outside.path().join("escape.pdf");
        let _ = std::fs::write(&outside_file, b"x");
        let Ok(mut backend) = Backend::headless(None, Some(jail.path().to_path_buf())) else {
            return;
        };
        let err = backend.open_document(&outside_file.display().to_string(), None, None);
        assert!(err.is_err());
    }
}
