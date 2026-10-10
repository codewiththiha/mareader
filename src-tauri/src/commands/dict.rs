//! The dictionary's invoke surface: pack rows, lifecycle, lookups.

use tauri::{AppHandle, Manager};

use crate::dict::{DictEntryWire, DictManager, PackStatus};

/// Every pack's row: what it is and where its download stands.
#[tauri::command]
pub async fn dict_packs_status(app: AppHandle) -> Result<Vec<PackStatus>, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<DictManager>().status(&app))
        .await
        .map_err(|error| format!("status worker: {error}"))
}

/// Start one pack's download; progress rides `dict-packs`.
#[tauri::command]
pub fn dict_pack_download(app: AppHandle, pack_id: String) -> Result<(), String> {
    app.state::<DictManager>()
        .begin_download(app.clone(), &pack_id)
}

/// Stop reading a pack; the partial stays for resume.
#[tauri::command]
pub fn dict_pack_pause(app: AppHandle, pack_id: String) -> Result<(), String> {
    app.state::<DictManager>().pause(&app, &pack_id);
    Ok(())
}

/// Continue a paused pack from its partial.
#[tauri::command]
pub fn dict_pack_resume(app: AppHandle, pack_id: String) -> Result<(), String> {
    app.state::<DictManager>().resume(&app, &pack_id)
}

/// Drop a pack's partial and job.
#[tauri::command]
pub fn dict_pack_cancel(app: AppHandle, pack_id: String) -> Result<(), String> {
    app.state::<DictManager>().cancel(&app, &pack_id);
    Ok(())
}

/// Take a pack out entirely: files, partial, and open handle.
#[tauri::command]
pub fn dict_pack_remove(app: AppHandle, pack_id: String) -> Result<(), String> {
    app.state::<DictManager>().remove(&app, &pack_id)
}

/// The hover's ask: a word carried between two languages,
/// senses ranked by the detected role.
#[tauri::command]
pub async fn dict_lookup(
    app: AppHandle,
    word: String,
    from: String,
    to: String,
    pos: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<DictEntryWire>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<DictManager>().lookup(
            &app,
            &word,
            &from,
            &to,
            pos.as_deref(),
            limit.unwrap_or(24),
        )
    })
    .await
    .map_err(|e| format!("lookup worker: {e}"))
}

/// The route's ask: a word on either side, or between two shores.
#[tauri::command]
pub async fn dict_search(
    app: AppHandle,
    ask: String,
    pack_ids: Option<Vec<String>>,
    // The language the ask is written in; `None` asks every side.
    from: Option<String>,
    // The language the answer is wanted in; `None` asks every side.
    to: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<DictEntryWire>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<DictManager>().search(
            &app,
            &ask,
            pack_ids,
            from.as_deref(),
            to.as_deref(),
            limit.unwrap_or(60),
        )
    })
    .await
    .map_err(|e| format!("search worker: {e}"))
}
