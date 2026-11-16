//! Synthetic update payloads for local terminal and test mocking.

use std::path::PathBuf;

use super::github::validate_https_url;
use super::notes::{default_mock_notes, parse_release_url, parse_summary};
use super::{InstallKind, MockMode, MockUpdate, ReleaseInfo, Status, UpdatePlatform};

/// Resolved mock state produced from a [`MockUpdate`] specification.
pub(super) struct BuiltMock {
    pub platform: UpdatePlatform,
    pub install: InstallKind,
    pub mode: MockMode,
    pub release: ReleaseInfo,
    pub status: Status,
    pub modal_open: bool,
}

/// Builds a self-contained mock release without touching disk or network.
pub(super) fn build(spec: MockUpdate, fallback_platform: UpdatePlatform) -> BuiltMock {
    let platform = spec.platform.unwrap_or(fallback_platform);
    let mode = spec.mode.unwrap_or(match platform {
        UpdatePlatform::Mac => MockMode::Notify,
        UpdatePlatform::Windows | UpdatePlatform::Linux => MockMode::Install,
    });
    let trimmed = spec.version.trim();
    let ver_raw = trimmed.strip_prefix('v').unwrap_or(trimmed);
    let version = if ver_raw.is_empty() {
        "99.0.0".to_owned()
    } else {
        ver_raw.to_owned()
    };
    let notes = spec.notes.unwrap_or_else(|| default_mock_notes(&version));
    let summary = parse_summary(&notes);
    let tag = format!("v{version}");
    let html_url = format!("https://github.com/codewiththiha/mareader2/releases/tag/{tag}");
    let download_url = spec
        .url
        .filter(|u| validate_https_url(u))
        .or_else(|| parse_release_url(&notes))
        .unwrap_or_else(|| html_url.clone());

    let install = mock_install(platform, mode);
    let can_install = platform != UpdatePlatform::Mac && install.can_self_update();
    let release = ReleaseInfo {
        version: version.clone(),
        tag,
        html_url,
        download_url: download_url.clone(),
        asset_url: can_install.then_some(download_url),
        asset_name: Some(mock_asset_name(platform, &version)),
        asset_size: Some(24 * 1024 * 1024),
        sha256: Some("0".repeat(64)),
        summary,
    };
    let (status, modal_open) = match mode {
        MockMode::Notify | MockMode::Install => (Status::Available, true),
        MockMode::Staged => (
            Status::ReadyToRestart {
                version,
                silent: true,
            },
            false,
        ),
    };
    BuiltMock {
        platform,
        install,
        mode,
        release,
        status,
        modal_open,
    }
}

pub(super) fn mock_install(platform: UpdatePlatform, mode: MockMode) -> InstallKind {
    match (platform, mode) {
        (UpdatePlatform::Mac, _) => InstallKind::NotifyOnly,
        (UpdatePlatform::Windows | UpdatePlatform::Linux, MockMode::Notify) => {
            InstallKind::PackageManaged
        }
        (UpdatePlatform::Windows, MockMode::Install | MockMode::Staged) => {
            InstallKind::WindowsNsis(PathBuf::from(
                "C:\\Users\\demo\\AppData\\Local\\Mareader\\mareader.exe",
            ))
        }
        (UpdatePlatform::Linux, MockMode::Install | MockMode::Staged) => {
            InstallKind::LinuxAppImage(PathBuf::from("/home/demo/Applications/Mareader.AppImage"))
        }
    }
}

fn mock_asset_name(platform: UpdatePlatform, version: &str) -> String {
    match platform {
        UpdatePlatform::Mac => format!("Mareader_{version}_aarch64.dmg"),
        UpdatePlatform::Windows => format!("mareader_{version}_x64-setup.exe"),
        UpdatePlatform::Linux => format!("mareader_{version}_x86_64.AppImage"),
    }
}
