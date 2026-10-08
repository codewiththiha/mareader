//! The downloader's app side: the bus any feature may listen on, and the
//! data directory every download lands under. The transport itself is
//! `download-core`; this is only the host it runs against.

use std::future::Future;
use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager};

use download_core::Host;

pub use download_core::{DownloadRequest, Downloads, Phase, Progress, ProgressHook};

/// Every phase change and throttled byte count rides this event; a feature
/// that wants a hook instead passes one to `Downloads::start`.
pub const PROGRESS_EVENT: &str = "download-progress";

/// The app-hosted downloader, as managed state.
pub type AppDownloads = Downloads;

/// Hosts downloads on Tauri: runtime tasks, bus events, app files.
#[derive(Clone)]
pub struct TauriHost {
    app: AppHandle,
}

impl TauriHost {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl Host for TauriHost {
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
        tauri::async_runtime::spawn(task);
    }

    fn publish(&self, progress: &Progress) {
        let _ = self.app.emit(PROGRESS_EVENT, progress);
    }

    fn data_dir(&self) -> Result<PathBuf, String> {
        self.app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app data dir: {e}"))
    }
}
