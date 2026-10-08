//! The generic download surface: start, pause, resume, cancel, poll.

use tauri::{AppHandle, Manager};

use crate::download::{AppDownloads, DownloadRequest, Progress, TauriHost};

/// Begin a download; progress arrives on `download-progress` under `id`.
#[tauri::command]
pub fn download_start(app: AppHandle, request: DownloadRequest) -> Result<(), String> {
    let host = TauriHost::new(app.clone());
    app.state::<AppDownloads>().start(&host, request, None)
}

/// Stop reading; the partial stays and resume continues from it.
#[tauri::command]
pub fn download_pause(app: AppHandle, id: String) {
    app.state::<AppDownloads>().pause(&id);
}

/// Continue a paused download from the bytes already on disk.
#[tauri::command]
pub fn download_resume(app: AppHandle, id: String) -> Result<(), String> {
    let host = TauriHost::new(app.clone());
    app.state::<AppDownloads>().resume(&host, &id)
}

/// Ask a running download to stop; the partial stays for a later start.
#[tauri::command]
pub fn download_cancel(app: AppHandle, id: String) {
    app.state::<AppDownloads>().cancel(&id);
}

/// Drop a download's record and its partial file.
#[tauri::command]
pub fn download_remove(app: AppHandle, id: String) {
    app.state::<AppDownloads>().remove(&id);
}

/// The current snapshot of one download.
#[tauri::command]
pub fn download_status(app: AppHandle, id: String) -> Option<Progress> {
    app.state::<AppDownloads>().status(&id)
}

/// Every download's snapshot, for a booting frontend.
#[tauri::command]
pub fn download_list(app: AppHandle) -> Vec<Progress> {
    app.state::<AppDownloads>().list()
}
