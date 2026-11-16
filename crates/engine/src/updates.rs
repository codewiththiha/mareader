//! Cross-platform release checking, summary parsing, and safe updates (L5).

pub mod github;
mod mock;
pub mod notes;
#[cfg(not(target_arch = "wasm32"))]
mod poll;
mod state;
#[cfg(not(target_arch = "wasm32"))]
mod worker;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use mareader_settings::UpdateSettings;
use serde::{Deserialize, Serialize};

pub use self::github::RELEASES_PAGE_URL;
pub use self::notes::{ReleaseSummary, Version, is_newer};
pub use self::state::State;

/// Operating system family for update UX and installer selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdatePlatform {
    Mac,
    Windows,
    Linux,
}

impl UpdatePlatform {
    /// Platform compiled into this binary.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    /// Parses `"mac"`, `"windows"`, `"linux"`, or `"gnome"`.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "mac" | "macos" | "darwin" => Some(Self::Mac),
            "windows" | "win" => Some(Self::Windows),
            "linux" | "gnome" => Some(Self::Linux),
            _ => None,
        }
    }
}

/// Whether this installation can replace its own binary without elevation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallKind {
    /// macOS unsigned distribution: notify with a browser download link.
    NotifyOnly,
    /// User-writable Linux AppImage pointed to by `APPIMAGE`.
    LinuxAppImage(PathBuf),
    /// Per-user Windows NSIS installation under `%LOCALAPPDATA%`.
    WindowsNsis(PathBuf),
    /// System or package-manager install (`.deb`, pacman, MSI).
    PackageManaged,
}

impl InstallKind {
    /// Whether in-app silent or interactive self-update is supported here.
    pub fn can_self_update(&self) -> bool {
        matches!(self, Self::LinuxAppImage(_) | Self::WindowsNsis(_))
    }
}

/// Verified metadata for a release newer than the running binary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag: String,
    pub html_url: String,
    /// Browser download link (override, platform asset, or `html_url`).
    pub download_url: String,
    /// Direct installer asset HTTPS URL used only by the self-updater.
    pub asset_url: Option<String>,
    pub asset_name: Option<String>,
    pub asset_size: Option<u64>,
    pub sha256: Option<String>,
    pub summary: ReleaseSummary,
}

/// Developer mock simulation mode for testing update flows locally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MockMode {
    /// Open the startup modal in notify-only / browser-download mode.
    Notify,
    /// Open the startup modal with self-update enabled ("Update & Restart").
    Install,
    /// Stage a silent background update with no startup modal.
    Staged,
}

impl MockMode {
    /// Parses `"notify"`, `"install"`, or `"staged"`.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "notify" | "link" | "modal" => Some(Self::Notify),
            "install" | "self" | "update" => Some(Self::Install),
            "staged" | "silent" | "ready" => Some(Self::Staged),
            _ => None,
        }
    }
}

/// Parameters for simulating a newly discovered release locally.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MockUpdate {
    pub version: String,
    pub notes: Option<String>,
    pub url: Option<String>,
    pub platform: Option<UpdatePlatform>,
    pub mode: Option<MockMode>,
}

/// Lifecycle status of update checks and staging.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Checking {
        manual: bool,
    },
    UpToDate,
    Available,
    Downloading {
        received: u64,
        total: Option<u64>,
    },
    ReadyToRestart {
        version: String,
        silent: bool,
    },
    Failed(String),
}

/// Commands routed to the engine update subsystem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Check { manual: bool },
    Install,
    Restart,
    Dismiss,
    ShowModal,
    OpenDownload,
    OpenNotes,
    Mock(MockUpdate),
}

/// Side effects the caller (`Session`) should perform after handling an action.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub changed: bool,
    pub close_settings: bool,
    pub pending_restart: bool,
    pub restart_ready: bool,
    pub toast: Option<String>,
}

/// Whether an automatic startup check is allowed by platform and user settings.
pub fn should_auto_check(
    platform: UpdatePlatform,
    install: &InstallKind,
    settings: &UpdateSettings,
) -> bool {
    match platform {
        UpdatePlatform::Mac => settings.check_on_startup,
        UpdatePlatform::Windows | UpdatePlatform::Linux => {
            settings.check_on_startup || (settings.auto_update && install.can_self_update())
        }
    }
}
