//! Synchronous stdio JSON-RPC 2.0 Model Context Protocol server.

use std::io::{self, BufRead, Write};

use mareader_engine::journal::Step;
use serde_json::{Value, json};

use crate::backend::Backend;
use crate::tools::{resource_definitions, tool_definitions};

const PROTOCOL_VERSION: &str = "2024-11-05";
const MAX_RPC_BYTES: usize = 1024 * 1024;

/// Stateful MCP JSON-RPC 2.0 handler backed by [`Backend`].
pub struct Server {
    backend: Backend,
}

impl Server {
    /// Wraps `backend` in an MCP JSON-RPC handler.
    pub fn new(backend: Backend) -> Self {
        Self { backend }
    }

    /// Handles one JSON-RPC line; returns `None` for notifications.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(why) => {
                return encode(err_response(Value::Null, -32700, &format!("parse: {why}")));
            }
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        // Notifications carry no `id` and expect no reply.
        let id = id?;
        let live = self.backend.is_live();
        let response = match method {
            "initialize" => ok_response(id, initialize_result(&self.backend)),
            "ping" => ok_response(id, json!({})),
            "tools/list" => ok_response(id, json!({ "tools": tool_definitions(live) })),
            "tools/call" => self.handle_tool_call(id, &params),
            "resources/list" => ok_response(id, json!({ "resources": resource_definitions() })),
            "resources/read" => self.handle_resource_read(id, &params),
            other => err_response(id, -32601, &format!("method not found: {other}")),
        };
        encode(response)
    }

    fn handle_tool_call(&mut self, id: Value, params: &Value) -> Value {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return err_response(id, -32602, "tools/call requires a string `name`");
        };
        let args = params.get("arguments").cloned().unwrap_or(Value::Null);
        match self.dispatch_tool(name, args) {
            Ok(payload) => ok_response(
                id,
                json!({
                    "content": format_tool_content(payload),
                    "isError": false,
                }),
            ),
            Err(why) => ok_response(
                id,
                json!({
                    "content": [{ "type": "text", "text": why }],
                    "isError": true,
                }),
            ),
        }
    }

    fn dispatch_tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        match name {
            "open_document" | "read_pages" | "get_outline" | "search_document" | "list_marks"
            | "annotate_text" | "export_notes" | "list_library" | "render_page" => {
                self.dispatch_reader_tool(name, &args)
            }
            "list_commands" => {
                let kind = args.get("kind").and_then(Value::as_str);
                let query = args.get("query").and_then(Value::as_str);
                self.backend.list_commands(kind, query)
            }
            "get_state" => self.backend.get_state(),
            "run_command" => {
                let cmd_id = args
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "run_command requires string `id`".to_owned())?;
                let params = args.get("params").cloned().unwrap_or_else(|| json!({}));
                let coalesce = args
                    .get("coalesce")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                self.backend.run_command(cmd_id, params, coalesce)
            }
            "run_batch" => {
                let raw = args
                    .get("commands")
                    .cloned()
                    .ok_or_else(|| "run_batch requires `commands` array".to_owned())?;
                let steps: Vec<Step> =
                    serde_json::from_value(raw).map_err(|e| format!("invalid `commands`: {e}"))?;
                self.backend.run_batch(steps)
            }
            "get_journal" => {
                let clear = args.get("clear").and_then(Value::as_bool).unwrap_or(false);
                self.backend.journal(clear)
            }
            "replay_journal" => {
                let raw = args
                    .get("steps")
                    .cloned()
                    .ok_or_else(|| "replay_journal requires `steps` array".to_owned())?;
                let steps: Vec<Step> =
                    serde_json::from_value(raw).map_err(|e| format!("invalid `steps`: {e}"))?;
                self.backend.replay(steps)
            }
            "ui_input" => {
                let actions = args
                    .get("actions")
                    .cloned()
                    .ok_or_else(|| "ui_input requires `actions` array".to_owned())?;
                self.backend.ui_input(actions)
            }
            "ui_screenshot" => {
                let path = args.get("path").and_then(Value::as_str);
                self.backend.ui_screenshot(path)
            }
            other => Err(format!("unknown tool {other:?}")),
        }
    }

    fn dispatch_reader_tool(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let pane = opt_u64(args, "pane");
        match name {
            "open_document" => {
                let path = args
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "open_document requires string `path`".to_owned())?;
                self.backend
                    .open_document(path, pane, opt_usize(args, "page"))
            }
            "read_pages" => {
                let start = opt_usize(args, "start_page").or_else(|| opt_usize(args, "page"));
                let end = opt_usize(args, "end_page").or(start);
                self.backend.read_pages(pane, start, end)
            }
            "get_outline" => self.backend.get_outline(pane),
            "search_document" => {
                let query = args
                    .get("query")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "search_document requires string `query`".to_owned())?;
                let max_hits = opt_usize(args, "max_hits").unwrap_or(100);
                self.backend.search_document(query, pane, max_hits)
            }
            "list_marks" => self.backend.list_marks(pane),
            "annotate_text" => {
                let page = opt_usize(args, "page")
                    .ok_or_else(|| "annotate_text requires integer `page`".to_owned())?;
                let text = args
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "annotate_text requires string `text`".to_owned())?;
                let pen = args.get("pen").and_then(Value::as_str);
                let note = args.get("note").and_then(Value::as_str);
                self.backend.annotate_text(page, text, pen, note, pane)
            }
            "export_notes" => {
                let format = args.get("format").and_then(Value::as_str);
                self.backend.export_notes(format, pane)
            }
            "list_library" => {
                let query = args.get("query").and_then(Value::as_str);
                let shelf = args.get("shelf").and_then(Value::as_str);
                self.backend.list_library(query, shelf)
            }
            "render_page" => {
                let page = opt_usize(args, "page");
                let scale = args
                    .get("scale")
                    .and_then(Value::as_f64)
                    .map_or(1.5, |s| s as f32);
                let path = args.get("path").and_then(Value::as_str);
                self.backend.render_page(pane, page, scale, path)
            }
            other => Err(format!("unknown reader tool {other:?}")),
        }
    }

    fn handle_resource_read(&mut self, id: Value, params: &Value) -> Value {
        let Some(uri) = params.get("uri").and_then(Value::as_str) else {
            return err_response(id, -32602, "resources/read requires string `uri`");
        };
        let payload = match uri {
            "mareader://commands" => self.backend.list_commands(None, None),
            "mareader://state" => self.backend.get_state(),
            "mareader://library" => self.backend.list_library(None, None),
            "mareader://document/outline" => self.backend.get_outline(None),
            "mareader://journal" => self.backend.journal(false),
            other => return err_response(id, -32602, &format!("unknown resource {other:?}")),
        };
        match payload {
            Ok(val) => {
                let text = serde_json::to_string_pretty(&val).unwrap_or_default();
                ok_response(
                    id,
                    json!({
                        "contents": [{
                            "uri": uri,
                            "mimeType": "application/json",
                            "text": text,
                        }]
                    }),
                )
            }
            Err(why) => err_response(id, -32000, &why),
        }
    }
}

/// Runs the synchronous MCP server loop over stdin and stdout until EOF.
pub fn run_stdio(backend: Backend) -> Result<(), String> {
    let mut server = Server::new(backend);
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| format!("stdin read: {e}"))?;
        if read == 0 {
            return Ok(());
        }
        if line.len() > MAX_RPC_BYTES {
            let reply = err_response(Value::Null, -32600, "JSON-RPC line too large");
            if let Some(encoded) = encode(reply) {
                writeln!(writer, "{encoded}").map_err(|e| format!("stdout write: {e}"))?;
                writer.flush().map_err(|e| format!("stdout flush: {e}"))?;
            }
            continue;
        }
        if let Some(reply) = server.handle_line(&line) {
            writeln!(writer, "{reply}").map_err(|e| format!("stdout write: {e}"))?;
            writer.flush().map_err(|e| format!("stdout flush: {e}"))?;
        }
    }
}

/// Emits an MCP `image` content block when `payload` contains `png_base64`.
fn format_tool_content(mut payload: Value) -> Vec<Value> {
    let png = payload
        .as_object_mut()
        .and_then(|obj| obj.remove("png_base64"))
        .and_then(|v| v.as_str().map(str::to_owned));
    let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
    match png {
        Some(data) => vec![
            json!({
                "type": "image",
                "data": data,
                "mimeType": "image/png",
            }),
            json!({ "type": "text", "text": text }),
        ],
        None => vec![json!({ "type": "text", "text": text })],
    }
}

fn initialize_result(backend: &Backend) -> Value {
    let mode = if backend.is_live() {
        "live"
    } else {
        "headless"
    };
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": {},
            "resources": {},
        },
        "serverInfo": {
            "name": "mareader-mcp",
            "version": env!("CARGO_PKG_VERSION"),
            "mode": mode,
        },
    })
}

fn opt_usize(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
}

fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

fn ok_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

fn encode(value: Value) -> Option<String> {
    serde_json::to_string(&value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call_tool(server: &mut Server, id: u64, name: &str, arguments: Value) -> Value {
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        });
        let raw = server.handle_line(&req.to_string()).unwrap_or_default();
        serde_json::from_str(&raw).unwrap_or(Value::Null)
    }

    #[test]
    fn headless_tools_omit_live_only_ui_tools() {
        let Ok(backend) = Backend::headless(None, None) else {
            return;
        };
        let mut server = Server::new(backend);
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let raw = server.handle_line(req).unwrap_or_default();
        let parsed: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        let names: Vec<&str> = parsed
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.get("name").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();
        assert!(names.contains(&"open_document"));
        assert!(names.contains(&"read_pages"));
        assert!(names.contains(&"render_page"));
        assert!(!names.contains(&"ui_screenshot"));
    }

    fn first_text(reply: &Value) -> &str {
        reply
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(Value::as_array)
            .and_then(|c| c.first())
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("")
    }

    fn is_error(reply: &Value) -> Option<bool> {
        reply
            .get("result")
            .and_then(|r| r.get("isError"))
            .and_then(Value::as_bool)
    }

    #[test]
    fn real_pdf_open_read_search_annotate_export_and_render() {
        let Ok(dir) = tempfile::tempdir() else {
            return;
        };
        let pdf_path = dir.path().join("demo.pdf");
        let text = "The quick brown fox jumps over the lazy dog";
        let pdf_bytes = mareader_pdf::sample::pdf(2, text);
        if std::fs::write(&pdf_path, pdf_bytes).is_err() {
            return;
        }
        let Ok(backend) = Backend::headless(None, Some(dir.path().to_path_buf())) else {
            return;
        };
        let mut server = Server::new(backend);

        let opened = call_tool(
            &mut server,
            1,
            "open_document",
            json!({ "path": pdf_path.display().to_string() }),
        );
        assert_eq!(is_error(&opened), Some(false));

        let read = call_tool(&mut server, 2, "read_pages", json!({ "page": 0 }));
        assert!(first_text(&read).contains("quick brown fox"));

        let hits = call_tool(
            &mut server,
            3,
            "search_document",
            json!({ "query": "brown fox" }),
        );
        assert!(first_text(&hits).contains("brown fox"));

        let marked = call_tool(
            &mut server,
            4,
            "annotate_text",
            json!({ "page": 0, "text": "quick brown fox", "note": "Verified pangram" }),
        );
        assert_eq!(is_error(&marked), Some(false));

        let exported = call_tool(
            &mut server,
            5,
            "export_notes",
            json!({ "format": "markdown" }),
        );
        assert!(first_text(&exported).contains("Verified pangram"));

        let rendered = call_tool(
            &mut server,
            6,
            "render_page",
            json!({ "page": 0, "scale": 1.0 }),
        );
        let first = rendered
            .get("result")
            .and_then(|r| r.get("content"))
            .and_then(Value::as_array)
            .and_then(|c| c.first());
        assert_eq!(
            first.and_then(|c| c.get("type")).and_then(Value::as_str),
            Some("image")
        );
        assert_eq!(
            first
                .and_then(|c| c.get("mimeType"))
                .and_then(Value::as_str),
            Some("image/png")
        );
        let b64 = first
            .and_then(|c| c.get("data"))
            .and_then(Value::as_str)
            .unwrap_or("");
        // PNG magic bytes always base64-encode starting with iVBORw0KGgo.
        assert!(b64.starts_with("iVBORw0KGgo"));
    }
}
