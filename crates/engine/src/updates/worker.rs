//! Desktop-only update worker: install detection, HTTPS checks, and staging.

use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use mareader_fetch::{Halt, Stopped, download_verified, get_text, sha256_hex};

use crate::session::Waker;

use super::github::{Arch, LATEST_API_URL, ParsedRelease, parse_release_json, parse_sha256sums};
use super::notes::is_newer;
use super::{InstallKind, ReleaseInfo, UpdatePlatform};

const MAX_API_BYTES: u64 = 512 * 1024;
const MAX_SUMS_BYTES: u64 = 128 * 1024;
const MAX_ASSET_BYTES: u64 = 300 * 1024 * 1024;
const API_HEADERS: &[(&str, &str)] = &[("Accept", "application/vnd.github+json")];

/// Messages sent from a background update thread back to [`super::State`].
pub(super) enum Msg {
    Checked {
        manual: bool,
        result: Result<Option<ReleaseInfo>, String>,
    },
    Progress {
        received: u64,
        total: Option<u64>,
    },
    Staged {
        version: String,
        path: PathBuf,
        sha256: String,
        silent: bool,
        restart_after: bool,
    },
    DownloadFailed {
        silent: bool,
        error: String,
    },
}

/// Active background check or download job.
pub(super) struct Worker {
    pub rx: Receiver<Msg>,
    pub halt: Arc<AtomicU8>,
}

impl Worker {
    pub fn cancel(&self) {
        self.halt.store(Halt::Cancel as u8, Ordering::Relaxed);
    }
}

/// Detects whether this desktop install can update without elevation.
pub(super) fn detect_install(platform: UpdatePlatform) -> InstallKind {
    match platform {
        UpdatePlatform::Mac => InstallKind::NotifyOnly,
        UpdatePlatform::Linux => detect_linux_appimage(),
        UpdatePlatform::Windows => detect_windows_nsis(),
    }
}

fn detect_linux_appimage() -> InstallKind {
    let Some(raw) = std::env::var_os("APPIMAGE") else {
        return InstallKind::PackageManaged;
    };
    let path = PathBuf::from(raw);
    let Some(parent) = path.parent() else {
        return InstallKind::PackageManaged;
    };
    if path.is_file() && dir_is_writable(parent) {
        InstallKind::LinuxAppImage(path)
    } else {
        InstallKind::PackageManaged
    }
}

fn detect_windows_nsis() -> InstallKind {
    let Ok(exe) = std::env::current_exe() else {
        return InstallKind::PackageManaged;
    };
    let Some(parent) = exe.parent() else {
        return InstallKind::PackageManaged;
    };
    let lower = parent.to_string_lossy().to_ascii_lowercase();
    if lower.contains("\\program files") || lower.contains("\\windows\\") {
        return InstallKind::PackageManaged;
    }
    if parent.join("uninstall.exe").is_file() && dir_is_writable(parent) {
        InstallKind::WindowsNsis(exe)
    } else {
        InstallKind::PackageManaged
    }
}

fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".mareader-probe-{}", std::process::id()));
    let ok = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    if ok {
        let _ = std::fs::remove_file(&probe);
    }
    ok
}

/// Removes leftover `.old` binaries and staged caches at or below `current`.
pub(super) fn cleanup_stale(cache_dir: Option<&Path>, current: &str) {
    if let Ok(exe) = std::env::current_exe() {
        let old = exe.with_extension("exe.old");
        if old.is_file() {
            let _ = std::fs::remove_file(old);
        }
    }
    let Some(root) = cache_dir.map(|d| d.join("updates")) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let ver = name.to_string_lossy();
        if !is_newer(&ver, current) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Spawns a background thread that checks GitHub Releases for a newer version.
pub(super) fn spawn_check(
    current: String,
    platform: UpdatePlatform,
    install: InstallKind,
    manual: bool,
    waker: Waker,
) -> Worker {
    let (tx, rx) = mpsc::channel();
    let halt = Arc::new(AtomicU8::new(Halt::Go as u8));
    let _ = std::thread::Builder::new()
        .name("mareader-update-check".into())
        .spawn(move || {
            let result = fetch_latest(&current, platform, &install);
            let _ = tx.send(Msg::Checked { manual, result });
            waker();
        });
    Worker { rx, halt }
}

fn fetch_latest(
    current: &str,
    platform: UpdatePlatform,
    install: &InstallKind,
) -> Result<Option<ReleaseInfo>, String> {
    let body = get_text(LATEST_API_URL, MAX_API_BYTES, API_HEADERS)?;
    let Some(mut parsed) = parse_release_json(&body, platform, Arch::current(), install)? else {
        return Ok(None);
    };
    if !is_newer(&parsed.info.version, current) {
        return Ok(None);
    }
    resolve_sha256(&mut parsed);
    Ok(Some(parsed.info))
}

fn resolve_sha256(parsed: &mut ParsedRelease) {
    if parsed.info.sha256.is_some() {
        return;
    }
    let (Some(name), Some(sums_url)) = (&parsed.info.asset_name, &parsed.sums_url) else {
        return;
    };
    if let Ok(sums) = get_text(sums_url, MAX_SUMS_BYTES, &[]) {
        parsed.info.sha256 = parse_sha256sums(&sums, name);
    }
}

/// Spawns a background thread that downloads and SHA-256-verifies `release`.
pub(super) fn spawn_download(
    release: ReleaseInfo,
    cache_dir: PathBuf,
    silent: bool,
    restart_after: bool,
    waker: Waker,
) -> Result<Worker, String> {
    let url = release.asset_url.clone().ok_or_else(|| {
        "This release has no downloadable installer for this platform.".to_owned()
    })?;
    let name = release
        .asset_name
        .clone()
        .ok_or_else(|| "Release asset filename is missing.".to_owned())?;
    let expected = release
        .sha256
        .clone()
        .ok_or_else(|| "Release SHA-256 checksum is unavailable; update manually.".to_owned())?;
    let (tx, rx) = mpsc::channel();
    let halt = Arc::new(AtomicU8::new(Halt::Go as u8));
    let halt_bg = Arc::clone(&halt);
    let version = release.version.clone();
    let _ = std::thread::Builder::new()
        .name("mareader-update-dl".into())
        .spawn(move || {
            run_download(
                DownloadJob {
                    url,
                    name,
                    expected,
                    version,
                    cache_dir,
                    silent,
                    restart_after,
                },
                &halt_bg,
                &tx,
                &waker,
            );
        });
    Ok(Worker { rx, halt })
}

struct DownloadJob {
    url: String,
    name: String,
    expected: String,
    version: String,
    cache_dir: PathBuf,
    silent: bool,
    restart_after: bool,
}

fn run_download(job: DownloadJob, halt: &AtomicU8, tx: &Sender<Msg>, waker: &Waker) {
    let dir = job.cache_dir.join("updates").join(&job.version);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        let _ = tx.send(Msg::DownloadFailed {
            silent: job.silent,
            error: format!("Could not prepare update folder: {e}"),
        });
        waker();
        return;
    }
    let final_path = dir.join(&job.name);
    if final_path.is_file()
        && sha256_hex(&final_path).is_ok_and(|h| h.eq_ignore_ascii_case(&job.expected))
    {
        let _ = tx.send(Msg::Staged {
            version: job.version,
            path: final_path,
            sha256: job.expected,
            silent: job.silent,
            restart_after: job.restart_after,
        });
        waker();
        return;
    }
    let part = dir.join(format!("{}.part", job.name));
    let mut last_step = 0u64;
    let res = download_verified(
        &job.url,
        &part,
        &job.expected,
        MAX_ASSET_BYTES,
        halt,
        &mut |received, total| {
            if received == 0 || received.saturating_sub(last_step) >= 128 * 1024 {
                last_step = received;
                let _ = tx.send(Msg::Progress { received, total });
                waker();
            }
        },
    );
    match res {
        Ok(()) => match std::fs::rename(&part, &final_path) {
            Ok(()) => {
                let _ = tx.send(Msg::Staged {
                    version: job.version,
                    path: final_path,
                    sha256: job.expected,
                    silent: job.silent,
                    restart_after: job.restart_after,
                });
            }
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                let _ = tx.send(Msg::DownloadFailed {
                    silent: job.silent,
                    error: format!("Could not stage update file: {e}"),
                });
            }
        },
        Err(Stopped::Halted(_)) => {}
        Err(Stopped::Failed(error)) => {
            let _ = tx.send(Msg::DownloadFailed {
                silent: job.silent,
                error,
            });
        }
    }
    waker();
}

/// Applies a verified staged binary on exit or immediately before restarting.
pub(super) fn apply_staged(
    install: &InstallKind,
    staged: &Path,
    expected_sha256: &str,
    state_dir: Option<&Path>,
    restart: bool,
) -> Result<(), String> {
    let actual = sha256_hex(staged).map_err(|e| format!("Staged update unreadable: {e}"))?;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        let _ = std::fs::remove_file(staged);
        return Err("Staged update failed integrity verification.".into());
    }
    match install {
        InstallKind::LinuxAppImage(target) => apply_appimage(staged, target, state_dir, restart),
        InstallKind::WindowsNsis(_) => apply_windows_nsis(staged, state_dir, restart),
        InstallKind::NotifyOnly | InstallKind::PackageManaged => {
            Err("This installation must be updated manually.".into())
        }
    }
}

fn release_instance_lock(state_dir: Option<&Path>) {
    if let Some(dir) = state_dir {
        let _ = std::fs::remove_file(dir.join("instance"));
    }
}

fn apply_appimage(
    staged: &Path,
    target: &Path,
    state_dir: Option<&Path>,
    restart: bool,
) -> Result<(), String> {
    let target = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    let parent = target
        .parent()
        .ok_or_else(|| "AppImage has no parent directory.".to_owned())?;
    let tmp = parent.join(format!(".mareader-update-{}.tmp", std::process::id()));
    std::fs::copy(staged, &tmp).map_err(|e| format!("Could not copy update: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        if let Err(e) = std::fs::set_permissions(&tmp, perms) {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("Could not mark update executable: {e}"));
        }
    }
    if let Err(e) = std::fs::rename(&tmp, &target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Could not replace AppImage: {e}"));
    }
    let _ = std::fs::remove_file(staged);
    if restart {
        release_instance_lock(state_dir);
        let mut cmd = Process::new(&target);
        // Drop the old AppImage's mount vars so the new bundle mounts fresh.
        for key in ["APPIMAGE", "APPDIR", "ARGV0", "OWD"] {
            cmd.env_remove(key);
        }
        cmd.spawn()
            .map_err(|e| format!("Updated AppImage, but failed to relaunch: {e}"))?;
    }
    Ok(())
}

/// Spawns cargo-packager's NSIS installer (`/S /NS`, plus `/R` on restart).
fn apply_windows_nsis(
    staged: &Path,
    state_dir: Option<&Path>,
    restart: bool,
) -> Result<(), String> {
    // Release lock first; NSIS replaces mareader.exe and `/R` relaunches it.
    release_instance_lock(state_dir);
    let mut cmd = Process::new(staged);
    cmd.arg("/S").arg("/NS");
    if restart {
        cmd.arg("/R");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
        .map_err(|e| format!("Could not launch silent installer: {e}"))?;
    Ok(())
}
