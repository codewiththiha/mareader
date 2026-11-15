//! `mareader-cli render`, `mareader-cli mcp`, and `mareader-cli ui` verbs.

use std::path::PathBuf;

use mareader_engine::inspect;
use mareader_engine::journal::Journal;
use mareader_mcp::backend::Backend;
use mareader_mcp::client::ControlClient;
use mareader_mcp::image_io::emit_rendered_page;
use serde_json::{Value, json};

use crate::driver::Runner;

const RENDER_USAGE: &str = "\
usage: mareader-cli [--state-dir <dir>] render [book] [--page N] [--scale F] [-o out.png]";

const MCP_USAGE: &str = "\
usage: mareader-cli mcp [--control-file <path>] [--control <port> --token <tok>] [--root <dir>]";

const UI_USAGE: &str = "\
usage: mareader-cli ui (--control-file <path> | --control <port> --token <tok>) <op> [args...]

ops:
  ping                          check live UI control server reachability
  state                         inspect live UI and engine state as JSON
  commands                      list live engine and UI commands
  exec <id> [json]              run an engine or ui.* command on the live window
  input <json>                  inject synthetic pointer/scroll/key/text actions
  screenshot [path.png]         capture desktop window PNG (file or base64)
  read-pages [start] [end]      extract text from 0-based open pages
  outline                       read open document outline / table of contents
  marks                         list bookmarks, highlights, and notes
  export-notes [md|txt|json]    export highlights and notes
  search <query>                inspect or run search hits on open document
  library [query]               list books, shelves, and watched folders
  render-page [out.png]         render current page headlessly as PNG
  journal [--clear]             fetch recorded session journal entries
  replay <script.jsonl>         replay JSONL steps against the live session";

/// Connection target for live desktop UI control.
#[derive(Default)]
struct EndpointArgs {
    file: Option<PathBuf>,
    port: Option<u16>,
    token: Option<String>,
}

impl EndpointArgs {
    fn connect(&self) -> Result<Option<ControlClient>, String> {
        if let Some(path) = &self.file {
            return ControlClient::from_file(path).map(Some);
        }
        match (self.port, self.token.as_ref()) {
            (Some(port), Some(token)) => Ok(Some(ControlClient::new(port, token.clone()))),
            (Some(_), None) => Err("--control requires --token (or use --control-file)".to_owned()),
            (None, Some(_)) => Err("--token requires --control <port>".to_owned()),
            (None, None) => Ok(None),
        }
    }
}

/// Headlessly renders one document page to PNG using CPU rasterization.
pub fn run_render(state_dir: Option<PathBuf>, rest: &[&str]) -> Result<Value, String> {
    let mut book: Option<String> = None;
    let mut page: Option<usize> = None;
    let mut scale: f32 = 1.5;
    let mut out: Option<PathBuf> = None;
    let mut idx = 0;
    while let Some(&arg) = rest.get(idx) {
        idx += 1;
        match arg {
            "-h" | "--help" => return Err(RENDER_USAGE.to_owned()),
            "--page" => {
                let raw = need(rest, &mut idx, arg)?;
                page = Some(
                    raw.parse::<usize>()
                        .map_err(|_| format!("bad page {raw:?}"))?,
                );
            }
            "--scale" => {
                let raw = need(rest, &mut idx, arg)?;
                scale = raw
                    .parse::<f32>()
                    .map_err(|_| format!("bad scale {raw:?}"))?;
            }
            "-o" | "--out" | "--output" => out = Some(PathBuf::from(need(rest, &mut idx, arg)?)),
            other => {
                if let Some(raw) = other.strip_prefix("--page=") {
                    page = Some(
                        raw.parse::<usize>()
                            .map_err(|_| format!("bad page {raw:?}"))?,
                    );
                } else if let Some(raw) = other.strip_prefix("--scale=") {
                    scale = raw
                        .parse::<f32>()
                        .map_err(|_| format!("bad scale {raw:?}"))?;
                } else if let Some(raw) = other.strip_prefix("--out=") {
                    out = Some(PathBuf::from(raw));
                } else if other.starts_with('-') {
                    return Err(format!("unknown render option {other:?}\n{RENDER_USAGE}"));
                } else if book.is_none() {
                    book = Some(other.to_owned());
                } else {
                    return Err(format!("unexpected render arg {other:?}\n{RENDER_USAGE}"));
                }
            }
        }
    }
    let mut runner = Runner::open(state_dir);
    if let Some(path) = book {
        let params = json!({ "path": path }).to_string();
        runner.exec("reader.open", &params)?;
    }
    let rendered = inspect::render_page(&runner.session, None, page, scale)?;
    emit_rendered_page(rendered, out.as_deref())
}

/// Runs the synchronous stdio MCP server (headless or attached live UI).
pub fn run_mcp(state_dir: Option<PathBuf>, rest: &[&str]) -> Result<(), String> {
    let mut ep = EndpointArgs::default();
    let mut root = None;
    let mut idx = 0;
    while let Some(&arg) = rest.get(idx) {
        idx += 1;
        match arg {
            "-h" | "--help" => return Err(MCP_USAGE.to_owned()),
            "--control-file" => ep.file = Some(PathBuf::from(need(rest, &mut idx, arg)?)),
            "--control" => {
                let raw = need(rest, &mut idx, arg)?;
                ep.port = Some(
                    raw.parse::<u16>()
                        .map_err(|_| format!("bad port {raw:?}"))?,
                );
            }
            "--token" => ep.token = Some(need(rest, &mut idx, arg)?.to_owned()),
            "--root" => root = Some(PathBuf::from(need(rest, &mut idx, arg)?)),
            other => {
                if let Some(p) = other.strip_prefix("--control-file=") {
                    ep.file = Some(PathBuf::from(p));
                } else if let Some(p) = other.strip_prefix("--control=") {
                    ep.port = Some(p.parse::<u16>().map_err(|_| format!("bad port {p:?}"))?);
                } else if let Some(t) = other.strip_prefix("--token=") {
                    ep.token = Some(t.to_owned());
                } else if let Some(r) = other.strip_prefix("--root=") {
                    root = Some(PathBuf::from(r));
                } else {
                    return Err(format!("unknown mcp option {other:?}\n{MCP_USAGE}"));
                }
            }
        }
    }
    let backend = match ep.connect()? {
        Some(client) => Backend::live(client, root)?,
        None => Backend::headless(state_dir, root)?,
    };
    mareader_mcp::server::run_stdio(backend)
}

/// Runs one request against a live Mareader desktop window over loopback TCP.
pub fn run_ui(rest: &[&str]) -> Result<Value, String> {
    let mut ep = EndpointArgs::default();
    let mut idx = 0;
    while let Some(&arg) = rest.get(idx) {
        match arg {
            "-h" | "--help" => return Err(UI_USAGE.to_owned()),
            "--control-file" => {
                idx += 1;
                ep.file = Some(PathBuf::from(need(rest, &mut idx, arg)?));
            }
            "--control" => {
                idx += 1;
                let raw = need(rest, &mut idx, arg)?;
                ep.port = Some(
                    raw.parse::<u16>()
                        .map_err(|_| format!("bad port {raw:?}"))?,
                );
            }
            "--token" => {
                idx += 1;
                ep.token = Some(need(rest, &mut idx, arg)?.to_owned());
            }
            other if other.starts_with("--control-file=") => {
                idx += 1;
                if let Some(p) = other.strip_prefix("--control-file=") {
                    ep.file = Some(PathBuf::from(p));
                }
            }
            other if other.starts_with("--control=") => {
                idx += 1;
                if let Some(p) = other.strip_prefix("--control=") {
                    ep.port = Some(p.parse::<u16>().map_err(|_| format!("bad port {p:?}"))?);
                }
            }
            other if other.starts_with("--token=") => {
                idx += 1;
                if let Some(t) = other.strip_prefix("--token=") {
                    ep.token = Some(t.to_owned());
                }
            }
            _ => break,
        }
    }
    let Some(client) = ep.connect()? else {
        return Err(format!(
            "ui requires --control-file <path> or --control <port> --token <tok>\n{UI_USAGE}"
        ));
    };
    let tail = rest.get(idx..).unwrap_or(&[]);
    dispatch_ui(&client, tail)
}

fn dispatch_ui(client: &ControlClient, args: &[&str]) -> Result<Value, String> {
    match args {
        ["ping"] => client.ping(),
        ["state"] => client.state(),
        ["commands"] => client.commands(),
        ["exec", id] => client.exec(id, json!({})),
        ["exec", id, raw] => {
            let params: Value =
                serde_json::from_str(raw).map_err(|e| format!("invalid params JSON: {e}"))?;
            client.exec(id, params)
        }
        ["input", raw] => {
            let actions: Value =
                serde_json::from_str(raw).map_err(|e| format!("invalid actions JSON: {e}"))?;
            client.input(actions)
        }
        ["screenshot"] => client.screenshot(None),
        ["screenshot", path] => client.screenshot(Some(path)),
        ["read-pages"] => client.query("read_pages", json!({})),
        ["read-pages", start] => {
            let s: usize = start.parse().map_err(|_| format!("bad page {start:?}"))?;
            client.query("read_pages", json!({ "start_page": s, "end_page": s }))
        }
        ["read-pages", start, end] => {
            let s: usize = start.parse().map_err(|_| format!("bad start {start:?}"))?;
            let e: usize = end.parse().map_err(|_| format!("bad end {end:?}"))?;
            client.query("read_pages", json!({ "start_page": s, "end_page": e }))
        }
        ["outline"] => client.query("outline", json!({})),
        ["marks"] => client.query("marks", json!({})),
        ["export-notes"] => client.query("export_notes", json!({ "format": "markdown" })),
        ["export-notes", fmt] => client.query("export_notes", json!({ "format": fmt })),
        ["search", query] => {
            let mut live = Backend::live(client.clone(), None)?;
            live.search_document(query, None, 100)
        }
        ["library"] => client.query("library", json!({})),
        ["library", query] => client.query("library", json!({ "query": query })),
        ["render-page"] => client.query("render_page", json!({})),
        ["render-page", path] => client.query("render_page", json!({ "path": path })),
        ["journal"] => client.journal(false),
        ["journal", "--clear"] => client.journal(true),
        ["replay", script] => {
            let text =
                std::fs::read_to_string(script).map_err(|e| format!("read {script}: {e}"))?;
            let steps = Journal::from_jsonl(&text)?;
            client.replay(&steps)
        }
        _ => Err(UI_USAGE.to_owned()),
    }
}

fn need<'a>(args: &[&'a str], idx: &mut usize, flag: &str) -> Result<&'a str, String> {
    let Some(&val) = args.get(*idx) else {
        return Err(format!("{flag} requires a value"));
    };
    *idx += 1;
    Ok(val)
}
