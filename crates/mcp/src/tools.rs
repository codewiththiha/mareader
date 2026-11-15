//! MCP tool and resource schema definitions advertised to clients.

use serde_json::{Value, json};

/// Builds the active tool catalog (`ui_*` tools appear only in live UI mode).
pub fn tool_definitions(live: bool) -> Value {
    let mut list = document_tools();
    list.extend(annotation_tools());
    list.extend(command_tools());
    if live {
        list.extend(live_ui_tools());
    }
    Value::Array(list)
}

/// Lists readable MCP JSON resources.
pub fn resource_definitions() -> Value {
    json!([
        {
            "uri": "mareader://state",
            "name": "Session state",
            "description": "Current route, panes, open documents, library counts, and UI state.",
            "mimeType": "application/json"
        },
        {
            "uri": "mareader://commands",
            "name": "Command catalog",
            "description": "All registered Mareader engine and UI commands with parameters.",
            "mimeType": "application/json"
        },
        {
            "uri": "mareader://library",
            "name": "Library catalog",
            "description": "Books, shelves, and watched folders in the Mareader library.",
            "mimeType": "application/json"
        },
        {
            "uri": "mareader://document/outline",
            "name": "Active document outline",
            "description": "Table of contents and active chapter for the focused document.",
            "mimeType": "application/json"
        },
        {
            "uri": "mareader://journal",
            "name": "Session journal",
            "description": "Recorded command journal entries for inspection or replay.",
            "mimeType": "application/json"
        }
    ])
}

fn document_tools() -> Vec<Value> {
    vec![
        tool(
            "open_document",
            "Open a PDF, EPUB, Markdown, or text file and wait until ready.",
            json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string", "description": "Path to the document file." },
                    "pane": { "type": "integer", "description": "Optional target pane id." },
                    "page": { "type": "integer", "description": "Optional 0-based page." }
                }
            }),
        ),
        tool(
            "read_pages",
            "Extract real text from one or more 0-based pages of an open document.",
            json!({
                "type": "object",
                "properties": {
                    "pane": { "type": "integer" },
                    "page": { "type": "integer", "description": "Single 0-based page." },
                    "start_page": { "type": "integer", "description": "First 0-based page." },
                    "end_page": { "type": "integer", "description": "Last page (max 20)." }
                }
            }),
        ),
        tool(
            "get_outline",
            "Read the table of contents / PDF outline and active chapter.",
            json!({
                "type": "object",
                "properties": { "pane": { "type": "integer" } }
            }),
        ),
        tool(
            "search_document",
            "Full-text search across the open document with page numbers and snippets.",
            json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "pane": { "type": "integer" },
                    "max_hits": { "type": "integer", "description": "Max hits (default 100)." }
                }
            }),
        ),
        tool(
            "render_page",
            "Render a document page headlessly to PNG (file path or MCP image).",
            json!({
                "type": "object",
                "properties": {
                    "page": { "type": "integer", "description": "0-based page (default current)." },
                    "pane": { "type": "integer" },
                    "scale": { "type": "number", "description": "Scale 0.25..4.0 (default 1.5)." },
                    "path": { "type": "string", "description": "Optional PNG output path." }
                }
            }),
        ),
    ]
}

fn annotation_tools() -> Vec<Value> {
    vec![
        tool(
            "list_marks",
            "List bookmarks, highlights, and notes on the open document.",
            json!({
                "type": "object",
                "properties": { "pane": { "type": "integer" } }
            }),
        ),
        tool(
            "annotate_text",
            "Highlight a passage on a 0-based page and optionally attach a note.",
            json!({
                "type": "object",
                "required": ["page", "text"],
                "properties": {
                    "page": { "type": "integer" },
                    "text": { "type": "string", "description": "Exact substring on the page." },
                    "pen": { "type": "string", "enum": ["full", "half", "underline"] },
                    "note": { "type": "string", "description": "Optional note text." },
                    "pane": { "type": "integer" }
                }
            }),
        ),
        tool(
            "export_notes",
            "Export highlights and notes for the open book as Markdown or JSON.",
            json!({
                "type": "object",
                "properties": {
                    "format": { "type": "string", "enum": ["markdown", "json"] },
                    "pane": { "type": "integer" }
                }
            }),
        ),
        tool(
            "list_library",
            "List books, shelves, and watched folders in the Mareader library.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Filter by title or path." },
                    "shelf": { "type": "string", "description": "Filter by shelf name or id." }
                }
            }),
        ),
    ]
}

fn command_tools() -> Vec<Value> {
    vec![
        tool(
            "list_commands",
            "List registered Mareader engine and UI commands with live enablement.",
            json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["all", "engine", "ui"] },
                    "query": { "type": "string" }
                }
            }),
        ),
        tool(
            "get_state",
            "Inspect current route, panes, open documents, library counts, and UI state.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "run_command",
            "Execute any registered Mareader engine or UI command by id.",
            json!({
                "type": "object",
                "required": ["id"],
                "properties": {
                    "id": { "type": "string" },
                    "params": { "type": "object" },
                    "coalesce": { "type": "string" }
                }
            }),
        ),
        tool(
            "run_batch",
            "Execute a sequence of {id, params} commands in order.",
            json!({
                "type": "object",
                "required": ["commands"],
                "properties": { "commands": { "type": "array", "items": { "type": "object" } } }
            }),
        ),
        tool(
            "get_journal",
            "Read recorded session journal entries and optionally clear them.",
            json!({ "type": "object", "properties": { "clear": { "type": "boolean" } } }),
        ),
        tool(
            "replay_journal",
            "Deterministically replay a list of {id, params} journal steps.",
            json!({
                "type": "object",
                "required": ["steps"],
                "properties": { "steps": { "type": "array", "items": { "type": "object" } } }
            }),
        ),
    ]
}

fn live_ui_tools() -> Vec<Value> {
    vec![
        tool(
            "ui_input",
            "Inject synthetic pointer, scroll, key, or text input into the live desktop window.",
            json!({
                "type": "object",
                "required": ["actions"],
                "properties": { "actions": { "type": "array", "items": { "type": "object" } } }
            }),
        ),
        tool(
            "ui_screenshot",
            "Capture the live desktop window as PNG (writes file or returns MCP image).",
            json!({
                "type": "object",
                "properties": { "path": { "type": "string" } }
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
    })
}
