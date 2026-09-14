//! The frontend half of the library's filesystem wire: one Tauri listener for the app's life
//! ([`install_import_bridge`]) and the typed `invoke` wrappers over the shell's commands.

pub mod arrange;
pub mod conflict;
pub mod covers;
pub mod duplicate;
pub mod import;
pub mod reveal;

pub use arrange::{
    ReadingData, SeamSide, also_show, answer_departure_return, ask_relink, ask_shelf_apart,
    cancel_departure, cancel_relink, cancel_shelf_apart, confirm_departure, create_shelf_and_enter,
    create_shelf_here, delete_shelf, file_many, memberships, move_many_to_shelf, nest_many,
    nest_shelf, purge_books, relink_dialog, relink_search_folder, rename_shelf,
    reorder_shelves_to_anchor, take_shelf_apart, take_shelf_apart_as_copies, unfile_books,
};
pub use covers::backfill_missing;
pub use duplicate::{duplicate_row, duplicate_rows, duplicate_shelf};
pub use reveal::{path_of_row, path_of_shelf, reveal_book, reveal_in_folder, reveal_shelf};
pub use import::{
    dismiss_task, ground_tracking, import_files, import_folder, migrate_store_layout,
    rescan_watched, restore_deleted_book, set_shelf_watch, shelf_watch, verify_library, verify_one,
    GroundWatch,
};

pub(super) fn file_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string()
}

/// The fallback is the path itself, because a root ("/", "C:\\") has no last segment to show.
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

pub(crate) fn toast(state: AppState, message: String) {
    state.ui.toast.set(Some(Toast::new(message)));
}

const PROGRESS_CHANNEL: &str = "library://progress";

const CMD_SCAN: &str = "scan_folder";
const CMD_VERIFY: &str = "verify_paths";
const CMD_STORE: &str = "store_books";
const CMD_DELETE: &str = "delete_stored";
const CMD_RELOCATE: &str = "relocate_stored";
const CMD_REVEAL: &str = "reveal_in_folder";

fn desktop_only() -> String {
    "Importing folders is only available in the desktop app.".to_string()
}

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

pub async fn scan_folder(
    task: &str,
    root: &str,
    opts: &FolderOpts,
) -> Result<Vec<FoundFile>, String> {
    call(CMD_SCAN, &ScanArgs { task, root, opts }).await
}

pub async fn verify_paths(paths: Vec<String>) -> Result<Vec<PathCheck>, String> {
    call(CMD_VERIFY, &PathsArgs { paths }).await
}

pub async fn store_books(
    task: &str,
    requests: &[StoreRequest],
) -> Result<Vec<StoreResult>, String> {
    call(CMD_STORE, &StoreArgs { task, requests }).await
}

/// A failure is the shell's own per-file answer, already a sentence; the caller decides where it goes.
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

/// `None` for the fingerprint leaves the row the pending mark the startup sweep finishes.
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

pub async fn relocate_stored(
    requests: &[RelocateRequest],
) -> Result<RelocateResult, String> {
    call(CMD_RELOCATE, &RelocateArgs { requests }).await
}

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
    default_path: Option<String>,
}

/// A picker is the one focus event the window gets that the app caused itself, and the listener that event reaches answers with a walk of every watched folder.
static PICKER_OPEN: AtomicBool = AtomicBool::new(false);
static PICKER_CLOSED: AtomicU64 = AtomicU64::new(0);

const PICKER_GRACE_MS: u64 = 1_000;

pub(crate) fn picker_focus() -> bool {
    if PICKER_OPEN.load(Ordering::Relaxed) {
        return true;
    }
    let closed = PICKER_CLOSED.load(Ordering::Relaxed);
    closed != 0 && now_ms().saturating_sub(closed) < PICKER_GRACE_MS
}

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

/// Must be called inside the app reactive owner: `tauri_listen` parks its closure in that owner, and a dropped closure would free the wasm function-table entry Tauri's JS still holds.
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
