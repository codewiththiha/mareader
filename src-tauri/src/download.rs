//! The downloader's app side: its runtime and data directory.

use std::future::Future;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use download_core::Host;

pub use download_core::{DownloadRequest, Downloads, Phase, Progress, ProgressHook};

/// The app-hosted downloader, as managed state.
pub type AppDownloads = Downloads;

/// Hosts downloads on Tauri: runtime tasks and app files.
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

    /// Nothing is broadcast: the features here pass a hook, and no
    /// window listens.
    fn publish(&self, _progress: &Progress) {}

    fn data_dir(&self) -> Result<PathBuf, String> {
        self.app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app data dir: {e}"))
    }
}
