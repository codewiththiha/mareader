//! The frontend half of the library's filesystem wire.
//!
//! This file is the phone line and nothing else:
//!
//!   * [`install_import_bridge`] — ONE Tauri listener for the app's life, which
//!     re-broadcasts the shell's progress beats as a window
//!     [`IMPORT_PROGRESS_EVENT`]. Same shape as `services::ai`'s chunk bridge
//!     and for the same reason: a per-mount listener would stack handlers whose
//!     closures die with their owner, and the import dock mounts and unmounts
//!     with the page.
//!   * the `invoke` wrappers — one per shell command, each turning a typed
//!     request into the wire types `library_core::wire` declares and parsing the
//!     answer back. Nothing above this module ever sees a `JsValue`.
//!   * the two native pickers, filtered to the format registry's own extension
//!     list.
//!
//! ## The verb convention
//!
//! A function that splits a batch into the half that may land now and the
//! half that owes a question is a `screen_*` — [`conflict::screen`], the
//! shelf-move departure's screen, the import's merge screen. The answer is
//! always a `(clean, asked)` pair, and the asked half always goes to a sheet
//! rather than being dropped: a screen that silently discarded its second
//! half is the vanishing placement the sheets exist to stop.
//!
//! The deciding is NOT here. Which files a scan adds, where they land and what
//! a rescan skips is `library_core`'s ledger; [`import`] runs it against the
//! shell's answers and writes the result to the library state, [`arrange`]
//! holds the moves a reader makes by hand (a drag between shelves, a shelf filed
//! inside another, a removal, a relink), [`duplicate`] is the second instance
//! of one row a right-click asks for, and [`conflict`] is the one question
//! both ask before a placement lands: does the level this is going to already
//! hold a book of this name? The rule itself is `library_core::conflict`'s —
//! pure, and host-tested — and [`conflict`] is the wiring between it and the
//! three answers the sheet offers.
//!
//! [`import`]: crate::services::library::import
//! [`arrange`]: crate::services::library::arrange
//! [`duplicate`]: crate::services::library::duplicate
//! [`conflict`]: crate::services::library::conflict

pub mod arrange;
pub mod conflict;
pub mod covers;
pub mod duplicate;
pub mod import;
pub mod reveal;

pub use arrange::{
    PurgeOpts, SeamSide, also_show, answer_departure_return, ask_relink, cancel_departure,
    cancel_relink, confirm_departure, create_shelf_and_enter, create_shelf_here, delete_shelf,
    file_many, memberships, move_many_to_shelf, nest_many, nest_shelf, purge_books, relink_dialog,
    relink_search_folder, rename_shelf, reorder_shelves_to_anchor, unfile_books,
};
pub use covers::backfill_missing;
pub use duplicate::{duplicate_row, duplicate_rows, duplicate_shelf};
pub use reveal::{path_of_row, path_of_shelf, reveal_book, reveal_in_folder, reveal_shelf};
pub use import::{
    dismiss_task, ground_tracking, import_files, import_folder, migrate_store_layout,
    rescan_watched, restore_deleted_book, set_shelf_watch, shelf_watch, verify_library, verify_one,
    ShelfWatch,
};

/// The last segment of a path, on either separator, with no trailing separator.
/// Empty only for a path that is nothing but separators — which is why the
/// callers that turn it into a label have a fallback.
pub(super) fn file_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string()
}

/// What a folder is called wherever the library names one — a dock card, a
/// menu row's sublabel, the shelf at a watched root: the last segment of its
/// path, which is the name the reader picked it by. One rule rather than one
/// spelling per surface, and the fallback is the path itself, because a root
/// ("/", "C:\\") has no last segment to show.
pub(crate) fn folder_label(root: &str) -> String {
    let name = file_name(root);
    if name.is_empty() {
        root.to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::folder_label;

    #[test]
    fn a_folder_is_called_by_the_name_it_was_picked_by() {
        assert_eq!(folder_label("/Users/me/Books"), "Books");
        assert_eq!(folder_label("/Users/me/Books/"), "Books");
        assert_eq!(folder_label("C:\\Users\\me\\Books"), "Books");
        assert_eq!(folder_label("/"), "/");
    }
}

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::JsValue;

use library_core::book::Fingerprint;
use library_core::folder::FolderOpts;
use library_core::scan::FoundFile;
use library_core::wire::{
    ImportProgress, PathCheck, RelocateRequest, RelocateResult, StoreRequest, StoreResult,
};

use leptos::prelude::*;

use crate::state::{AppState, Toast};
use crate::time::now_ms;

pub use crate::events::IMPORT_PROGRESS_EVENT;

/// Put one sentence on the app's single toast slot.
///
/// The one spelling of that write for every library surface — the services,
/// the sheets and the menus — so a failure is delivered the same way wherever
/// it happened and no screen grows its own private route to the slot.
pub(crate) fn toast(state: AppState, message: String) {
    state.ui.toast.set(Some(Toast::new(message)));
}

/// The Tauri channel the shell emits progress on. Mirrors
/// `PROGRESS_EVENT` in `src-tauri/src/commands/library.rs`; the payload it
/// carries is [`ImportProgress`], which both sides get from `library_core::wire`
/// and so cannot drift.
const PROGRESS_CHANNEL: &str = "library://progress";

/// The shell command names. One table, so a rename on either side is a diff in
/// one file rather than a string that stops matching.
const CMD_SCAN: &str = "scan_folder";
const CMD_VERIFY: &str = "verify_paths";
const CMD_STORE: &str = "store_books";
const CMD_DELETE: &str = "delete_stored";
const CMD_RELOCATE: &str = "relocate_stored";
const CMD_REVEAL: &str = "reveal_in_folder";

/// What every command here answers when there is no shell to answer: the same
/// wording the open dialog uses, because from the reader's side it is the same
/// situation.
fn desktop_only() -> String {
    "Importing folders is only available in the desktop app.".to_string()
}

/// One `invoke`, typed at both ends.
///
/// `A` is serialized to the argument object the command expects and `T` parsed
/// back out of its answer, so the wire shape lives in `library_core::wire` and
/// the reflection lives here — the two things that used to be spread across
/// every call site.
async fn call<A: Serialize, T: DeserializeOwned>(cmd: &str, args: &A) -> Result<T, String> {
    if !tauri_bridge::has_tauri() {
        return Err(desktop_only());
    }
    let args = serde_wasm_bindgen::to_value(args)
        .map_err(|e| format!("{cmd}: could not encode the request ({e})"))?;
    let value = tauri_bridge::invoke(cmd, args)
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{cmd} failed: {e:?}")))?;
    serde_wasm_bindgen::from_value(value)
        .map_err(|e| format!("{cmd}: the shell answered something unparseable ({e})"))
}

#[derive(Serialize)]
struct ScanArgs<'a> {
    task: &'a str,
    root: &'a str,
    opts: &'a FolderOpts,
}

#[derive(Serialize)]
struct PathsArgs {
    paths: Vec<String>,
}

#[derive(Serialize)]
struct StoreArgs<'a> {
    task: &'a str,
    requests: &'a [StoreRequest],
}

#[derive(Serialize)]
struct RelocateArgs<'a> {
    requests: &'a [RelocateRequest],
}

#[derive(Serialize)]
struct PathArgs<'a> {
    path: &'a str,
}

/// Walk `root` and measure every file `opts` admits. `task` is the caller's id
/// for the run; it comes back on every progress beat.
pub async fn scan_folder(
    task: &str,
    root: &str,
    opts: &FolderOpts,
) -> Result<Vec<FoundFile>, String> {
    call(CMD_SCAN, &ScanArgs { task, root, opts }).await
}

/// Re-measure addresses the library already holds. One row per path, in order.
pub async fn verify_paths(paths: Vec<String>) -> Result<Vec<PathCheck>, String> {
    call(CMD_VERIFY, &PathsArgs { paths }).await
}

/// Copy files into the app's store. One result per request, so a single locked
/// file costs the reader that file and not the batch.
pub async fn store_books(
    task: &str,
    requests: &[StoreRequest],
) -> Result<Vec<StoreResult>, String> {
    call(CMD_STORE, &StoreArgs { task, requests }).await
}

/// Copy ONE file into the app's store, answering with the stored address.
///
/// The single-file form of [`store_books`], for the two places that copy
/// outside a batch — a restore's re-measured file and a relink's new source —
/// which used to each hand-roll the request, the result match and the same
/// two error sentences. A failure is the shell's own per-file answer, already
/// a sentence; the caller decides where it goes (a dock card, a toast).
pub(crate) async fn copy_one_to_store(task: &str, path: &str, id: &str) -> Result<String, String> {
    let requests = [StoreRequest {
        path: path.to_string(),
        id: id.to_string(),
    }];
    match store_books(task, &requests).await {
        Ok(results) => match results.into_iter().next() {
            Some(result) if result.is_ok() => Ok(result.store),
            Some(result) => Err(result
                .error
                .unwrap_or_else(|| "Could not copy that file.".to_string())),
            None => Err("Could not copy that file.".to_string()),
        },
        Err(message) => Err(message),
    }
}

/// Copy ONE file into the store and take the copy's own measurement: the
/// stored address, and its fingerprint — `None` when the copy could not be
/// weighed, which leaves the row the pending mark the startup sweep finishes.
///
/// The composition a departure, a merge's copy answer and a single-file
/// landing all ride, spelled once: copy FIRST, then measure the COPY rather
/// than the source, because a stored row's identity is the copy's fingerprint
/// and the source file's stays free for the folders that read it — the
/// departure rule's arithmetic, in the one place it can be got right.
pub(crate) async fn copy_and_measure(
    task: &str,
    path: &str,
    id: &str,
) -> Result<(String, Option<Fingerprint>), String> {
    let store = copy_one_to_store(task, path, id).await?;
    let measured = verify_paths(vec![store.clone()])
        .await
        .ok()
        .and_then(|checks| checks.into_iter().next())
        .and_then(|check| check.fingerprint());
    Ok((store, measured))
}

/// Move stored copies out of the old flat store into their own item folders.
///
/// One row per request, plus the store root the shell moved them inside — the
/// frontend cannot compute that root itself, and it needs it to recognise a copy
/// that has not been moved yet.
pub async fn relocate_stored(
    requests: &[RelocateRequest],
) -> Result<RelocateResult, String> {
    call(CMD_RELOCATE, &RelocateArgs { requests }).await
}

/// Ask the OS file manager to reveal a path: the item selected inside its
/// folder on the platforms that have the verb, the containing folder on the
/// one that has not.
///
/// The ok side is the absence of news and is never parsed — a file manager
/// that opened is its own report — and the error side arrives a sentence
/// already, which the caller puts on a toast: a reveal is a courtesy, and
/// its failure changes no state.
pub(crate) async fn reveal_path(path: String) -> Result<(), String> {
    let args = serde_wasm_bindgen::to_value(&PathArgs { path: &path })
        .map_err(|e| format!("reveal: could not encode the request ({e})"))?;
    tauri_bridge::invoke(CMD_REVEAL, args)
        .await
        .map_err(|e| {
            e.as_string()
                .unwrap_or_else(|| format!("The file manager did not open: {e:?}"))
        })
        .map(|_| ())
}

/// Delete a copy the app made, when the book it belonged to is removed. Fire
/// and forget: a store file that outlives its book wastes disk and nothing
/// else, and there is no version of this where the reader should see an error.
pub fn delete_stored(path: &str) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    let args = match serde_wasm_bindgen::to_value(&PathArgs { path }) {
        Ok(args) => args,
        Err(_) => return,
    };
    let path = path.to_string();
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = tauri_bridge::invoke(CMD_DELETE, args).await {
            let detail = e.as_string().unwrap_or_else(|| format!("{e:?}"));
            web_sys::console::warn_1(&format!("[library] could not delete {path}: {detail}").into());
        }
    });
}

/// The native multi-file picker, filtered to the formats the library holds.
///
/// The filter is the format registry's own extension list — the same source the
/// open dialog and the drag-drop admission read — so a fourth format appears in
/// all three at once. A cancel answers with an empty list rather than an error:
/// "the reader changed their mind" is not a failure and must not raise a toast.
pub async fn pick_documents() -> Result<Vec<String>, String> {
    pick(Options {
        directory: false,
        multiple: true,
        filter: true,
        default_path: None,
    })
    .await
    .map(|paths| paths.unwrap_or_default())
}

/// The native multi-file picker, rooted at a folder the library already knows.
///
/// "Open file picker here": the reader is looking at a watched folder's shelf and
/// wants one file out of it, without the sheet and without its filters. An
/// explicit pick is an explicit choice, so it bypasses the format set and the
/// size threshold — the format gate stays, because a file the reader cannot open
/// is not a book whatever they meant.
pub async fn pick_documents_in(default_path: String) -> Result<Vec<String>, String> {
    pick(Options {
        directory: false,
        multiple: true,
        filter: true,
        default_path: Some(default_path),
    })
    .await
    .map(|paths| paths.unwrap_or_default())
}

/// The native directory picker, for an import's folder.
pub async fn pick_folder() -> Result<Option<String>, String> {
    let paths = pick(Options {
        directory: true,
        multiple: false,
        filter: false,
        default_path: None,
    })
    .await?;
    Ok(paths.and_then(|p| p.into_iter().next()))
}

struct Options {
    directory: bool,
    multiple: bool,
    filter: bool,
    /// Where the picker opens. The shell's own dialog option, and the difference
    /// between "pick a file" and "pick a file from the folder you are looking at".
    default_path: Option<String>,
}

/// Whether one of the app's own native pickers is up, and the stamp of the last
/// one that closed.
///
/// A picker is the one focus event the window gets that the app caused itself:
/// a native dialog takes the window's place and hands it back, and the handback
/// is a `tauri://focus` like any other. Which matters because the listener that
/// event reaches answers with a walk of every watched folder — a walk of the
/// very ground the dialog was opened to pick, started one tick before the import
/// that was going to walk it properly.
static PICKER_OPEN: AtomicBool = AtomicBool::new(false);
static PICKER_CLOSED: AtomicU64 = AtomicU64::new(0);

/// How long after a picker closes its focus still counts as the picker's. The
/// handback and the dialog's answer are two messages on their way to the same
/// thread, and the grace is the room they need to arrive in either order.
const PICKER_GRACE_MS: u64 = 1_000;

/// Whether the window's current focus is the app's own picker handing it back.
///
/// The rescan's one guard against walking a folder the reader is about to
/// import by name — see `import::verify::run_watched`. Only the WALK waits for a
/// focus that means it; the measure pass, which is what marks a book `missing`
/// when its file died while a dialog was up, runs on every focus whatever.
pub(crate) fn picker_focus() -> bool {
    if PICKER_OPEN.load(Ordering::Relaxed) {
        return true;
    }
    let closed = PICKER_CLOSED.load(Ordering::Relaxed);
    // No stamp, no answer: off wasm the clock is inert at zero, and reading a
    // zero stamp as "closed this instant" would be a rescan that never runs.
    closed != 0 && now_ms().saturating_sub(closed) < PICKER_GRACE_MS
}

/// One `__TAURI__.dialog.open` call. Returns `None` on cancel.
async fn pick(options: Options) -> Result<Option<Vec<String>>, String> {
    if !tauri_bridge::has_tauri() {
        return Err(desktop_only());
    }
    let opts = JsValue::from(js_sys::Object::new());
    set(&opts, "multiple", &JsValue::from(options.multiple));
    set(&opts, "directory", &JsValue::from(options.directory));
    if let Some(default_path) = options.default_path.as_deref() {
        set(&opts, "defaultPath", &JsValue::from_str(default_path));
    }
    if options.filter {
        // One filter row naming every extension the registry knows, rather than
        // a row per format: the picker's job is "documents", not "which of the
        // three did you mean".
        let filter = JsValue::from(js_sys::Object::new());
        set(&filter, "name", &JsValue::from_str("Documents"));
        let exts = js_sys::Array::new();
        for ext in reader_core::format::extensions() {
            exts.push(&JsValue::from_str(ext));
        }
        set(&filter, "extensions", &exts);
        let filters = js_sys::Array::new();
        filters.push(&filter);
        set(&opts, "filters", &filters);
    }

    // Stamped around the await and not around the whole function: the dialog is
    // the only part of this that takes the window's focus away, and a picker
    // that failed to open never took it and so never owes one back.
    PICKER_OPEN.store(true, Ordering::Relaxed);
    let opened = tauri_bridge::open(opts).await;
    PICKER_OPEN.store(false, Ordering::Relaxed);
    PICKER_CLOSED.store(now_ms(), Ordering::Relaxed);
    let value = opened.map_err(|e| format!("Dialog failed: {}", describe(e)))?;
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    if let Some(one) = value.as_string() {
        return Ok(Some(vec![one]));
    }
    if js_sys::Array::is_array(&value) {
        let paths = js_sys::Array::from(&value)
            .iter()
            .filter_map(|v| v.as_string())
            .filter(|p| !p.is_empty())
            .collect();
        return Ok(Some(paths));
    }
    Ok(None)
}

fn set(target: &JsValue, key: &str, value: &JsValue) {
    _ = js_sys::Reflect::set(target, &JsValue::from_str(key), value);
}

fn describe(error: JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| format!("{error:?}"))
}

/// Register the ONE Tauri progress listener for the app's life and re-broadcast
/// every beat as a window [`IMPORT_PROGRESS_EVENT`].
///
/// Must be called inside the app reactive owner (the app root installs it next
/// to the AI bridge): `tauri_listen` parks its closure in that owner, and a
/// dropped closure would free the wasm function-table entry Tauri's JS still
/// holds. Outside Tauri this is a no-op — there is no shell to import from, and
/// the wasm-bindgen shim would throw on a missing global.
pub fn install_import_bridge() {
    if !tauri_bridge::has_tauri() {
        return;
    }
    crate::services::tauri_listen(PROGRESS_CHANNEL, move |ev: web_sys::Event| {
        let value: &JsValue = ev.as_ref();
        let Ok(payload) = js_sys::Reflect::get(value, &"payload".into()) else {
            return;
        };
        match serde_wasm_bindgen::from_value::<ImportProgress>(payload) {
            Ok(beat) => crate::events::dispatch_typed_event(IMPORT_PROGRESS_EVENT, &beat),
            Err(e) => {
                web_sys::console::warn_1(&format!("[library] bad progress payload: {e}").into());
            }
        }
    });
}
