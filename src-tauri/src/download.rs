//! The generic downloader, hosted on the app's event bus.

use std::future::Future;
use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager};

use download_core::Host;

pub use download_core::{Downloads, Phase, Progress, Receipt};

/// Every phase change and throttled byte count rides this event.
pub const PROGRESS_EVENT: &str = "download-progress";

/// The app-hosted downloader, as managed state.
pub type AppDownloads = Downloads;

/// Hosts downloads on Tauri: runtime tasks and bus events.
#[derive(Clone)]
pub struct TauriHost {
    app: AppHandle,
}

impl TauriHost {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    /// Where a feature's downloads land: `<app data>/<feature>`.
    pub fn feature_dir(&self, feature: &str) -> Result<PathBuf, String> {
        self.app
            .path()
            .app_data_dir()
            .map(|dir| dir.join(feature))
            .map_err(|e| format!("app data dir: {e}"))
    }
}

impl Host for TauriHost {
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
        tauri::async_runtime::spawn(task);
    }

    fn publish(&self, progress: &Progress) {
        let _ = self.app.emit(PROGRESS_EVENT, progress);
    }
}
