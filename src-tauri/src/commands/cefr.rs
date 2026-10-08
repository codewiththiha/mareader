//! The dataset's invoke surface: status, download, cancel, remove, lookup.

use tauri::{AppHandle, Manager};

use crate::cefr::{CefrManager, DatasetStatus};

/// The dataset's phase, probed from disk on the first ask of a run.
#[tauri::command]
pub async fn cefr_dataset_status(app: AppHandle) -> Result<DatasetStatus, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<CefrManager>().status(&app))
        .await
        .map_err(|e| format!("status worker: {e}"))?
}

/// Start the resumable download; progress rides `cefr-dataset-progress`.
#[tauri::command]
pub fn cefr_dataset_download(app: AppHandle) -> Result<(), String> {
    app.state::<CefrManager>().begin_download(app.clone())
}

/// Stop a running download; the partial stays for the next resume.
#[tauri::command]
pub fn cefr_dataset_cancel(app: AppHandle) {
    app.state::<CefrManager>().cancel(&app);
}

/// The dataset's POS verdict for one word in its sentence, at click time.
#[tauri::command]
pub fn cefr_pos_of(
    app: AppHandle,
    word: String,
    context: String,
) -> Result<Option<crate::cefr::PosAnswer>, String> {
    app.state::<CefrManager>().pos_of(&app, &word, &context)
}

/// Stop reading the stream; the partial stays for a resume.
#[tauri::command]
pub fn cefr_dataset_pause(app: AppHandle) {
    app.state::<CefrManager>().pause_download(&app);
}

/// Continue a paused download from the bytes already on disk.
#[tauri::command]
pub fn cefr_dataset_resume(app: AppHandle) -> Result<(), String> {
    app.state::<CefrManager>().resume_download(&app)
}

/// Delete the dataset and its partials.
#[tauri::command]
pub fn cefr_dataset_remove(app: AppHandle) -> Result<(), String> {
    app.state::<CefrManager>().remove(&app)
}

/// Levels for the words, aligned with the input; `None` where the dataset
/// has no answer.
#[tauri::command]
pub async fn cefr_levels(app: AppHandle, words: Vec<String>) -> Result<Vec<Option<f64>>, String> {
    // sqlite is blocking: the query leaves the async pool.
    tauri::async_runtime::spawn_blocking(move || app.state::<CefrManager>().levels(&app, &words))
        .await
        .map_err(|e| format!("lookup worker: {e}"))?
}
