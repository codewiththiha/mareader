//! Document lifecycle: open, outline, teardown, covers, OS-file handoff.

use crate::bridge;
use crate::types::{CoverResult, OpenResult, OutlineNode};

use super::{guard_pdf_reader, require_pdf_reader, resolve, EngineError};

pub async fn open(path: &str) -> Result<OpenResult, EngineError> {
    require_pdf_reader()?;
    let value = bridge::open(path).await;
    resolve::<OpenResult>(value, "open")
}

/// `{ok:true, outline}` — engine.resolveOutline.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutlinePayload {
    outline: Vec<OutlineNode>,
}

/// The open document's chapter tree, flattened. The open flow asks for this
/// AFTER the reader is up: resolving every outline destination is a per-entry
/// worker round trip, and holding `open` hostage to it was most of the
/// document-opening lag on textbook-sized outlines.
///
/// INVARIANT: `Ok(empty)` means "no engine, or no outline in this book" —
/// never an error. `outline_panel` treats empty as "no chapters", which is
/// correct in both cases; a genuine engine failure still surfaces as `Err`.
pub async fn outline() -> Result<Vec<OutlineNode>, EngineError> {
    if !guard_pdf_reader() {
        return Ok(Vec::new());
    }
    let value = bridge::resolve_outline().await;
    let payload: OutlinePayload = resolve(value, "resolveOutline")?;
    Ok(payload.outline)
}

/// Tear the engine document down (used when returning to the library shelf).
/// Also drops the Rust-owned search index for the document.
pub async fn destroy() {
    super::search::clear_index();
    let _ = bridge::destroy().await;
}

/// Render page 1 of the book at `path` to a small JPEG for the library
/// shelf's book cover. Works whether or not that book is the open document.
pub async fn cover_data_url(path: &str, max_width: f64) -> Result<CoverResult, EngineError> {
    require_pdf_reader()?;
    let value = bridge::cover_data_url(path, max_width).await;
    resolve::<CoverResult>(value, "cover")
}

/// Collect the pending OS-opened PDF path from the backend (double-click,
/// "Open with", default-app launch), if any. Consumes it, so a stray double
/// wake-up can never open the same file twice. Resolves None (never errors)
/// outside Tauri and whenever the backend has nothing queued.
pub async fn take_pending_file() -> Option<String> {
    if !bridge::has_tauri() || !bridge::has_pdf_reader() {
        return None;
    }
    let value = bridge::take_pending_file().await;
    value.as_string().filter(|s| !s.is_empty())
}
