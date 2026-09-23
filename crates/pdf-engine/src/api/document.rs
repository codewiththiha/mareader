//! Document lifecycle: open, outline, teardown, covers, OS-file handoff.

use crate::bridge;
use crate::types::{CoverResult, OpenResult, OutlineEntry};

use super::{EngineError, guard_pdf_reader, require_pdf_reader, resolve};

pub async fn open(path: &str) -> Result<OpenResult, EngineError> {
    require_pdf_reader()?;
    let value = bridge::open(path).await;
    let open: OpenResult = resolve(value, "open")?;
    // The search index is scoped to the document's CONTENT identity before
    // anything can query it: a retained index built for these exact bytes is
    // adopted by the build below instead of re-extracted, and any other
    // book's index is dropped now rather than at the first search.
    super::search::scope_to_document(open.fingerprint.as_deref(), path, open.num_pages);
    Ok(open)
}

/// `{ok:true, outline}` — engine.resolveOutline.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutlinePayload {
    outline: Vec<OutlineEntry>,
}

/// The open document's chapter tree, flattened into wire entries.
///
/// Entries, not outline nodes: which page a chapter may be jumped to is the
/// reader's business (it clamps against the page count it knows and drops what
/// never resolved), and the engine's job ends at reporting what the file said. The open flow asks for this
/// AFTER the reader is up: resolving every outline destination is a per-entry
/// worker round trip, and holding `open` hostage to it was most of the
/// document-opening lag on textbook-sized outlines.
///
/// INVARIANT: `Ok(empty)` means "no engine, or no outline in this book" —
/// never an error. `outline_panel` treats empty as "no chapters", which is
/// correct in both cases; a genuine engine failure still surfaces as `Err`.
pub async fn outline() -> Result<Vec<OutlineEntry>, EngineError> {
    if !guard_pdf_reader() {
        return Ok(Vec::new());
    }
    let value = bridge::resolve_outline().await;
    let payload: OutlinePayload = resolve(value, "resolveOutline")?;
    Ok(payload.outline)
}

/// Tear the engine document down (used when returning to the library shelf).
///
/// The Rust-owned search index deliberately SURVIVES: it is keyed by the
/// document's content fingerprint, so reopening the same book adopts it
/// instead of re-extracting every page (a rebuild per open/close cycle
/// ratchets the wasm heap, which never shrinks back). The next different
/// document's open drops it ([`super::search::scope_to_document`]).
pub async fn destroy() {
    let _ = bridge::destroy().await;
}

/// Render page 1 of the book at `path` to a small JPEG for the library
/// shelf's book cover. Works whether or not that book is the open document.
pub async fn cover_data_url(path: &str, max_width: f64) -> Result<CoverResult, EngineError> {
    require_pdf_reader()?;
    let value = bridge::cover_data_url(path, max_width).await;
    resolve::<CoverResult>(value, "cover")
}

/// Collect the pending OS-opened path from the backend (double-click,
/// "Open with", default-app launch), if any. Consumes it, so a stray double
/// wake-up can never open the same file twice. Resolves None (never errors)
/// outside Tauri and whenever the backend has nothing queued.
///
/// The command is Tauri's, not pdf.js's. A shelf boot has not loaded
/// `PDFReader`, and that must not drop the file the OS just handed over. When
/// the engine is present the existing bridge path stays, so a reader page
/// behaves as before.
pub async fn take_pending_file() -> Option<String> {
    if !tauri_bridge::has_tauri() {
        return None;
    }
    if bridge::has_pdf_reader() {
        let value = bridge::take_pending_file().await;
        return value.as_string().filter(|s| !s.is_empty());
    }
    let value = tauri_bridge::invoke("take_pending_file", js_sys::Object::new().into())
        .await
        .ok()?;
    value.as_string().filter(|s| !s.is_empty())
}
