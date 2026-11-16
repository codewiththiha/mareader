//! Engine-owned update state machine polled each frame by `Session`.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use mareader_settings::UpdateSettings;

use crate::host::Host;
use crate::session::Waker;

use super::github::validate_https_url;
use super::mock;
#[cfg(not(target_arch = "wasm32"))]
use super::worker;
use super::{
    Action, InstallKind, MockMode, MockUpdate, Outcome, ReleaseInfo, Status, UpdatePlatform,
    should_auto_check,
};

#[cfg(not(target_arch = "wasm32"))]
pub(super) struct Staged {
    pub path: PathBuf,
    pub sha256: String,
}

/// Engine-owned update state polled each frame by [`crate::session::Session`].
pub struct State {
    current_version: String,
    platform: UpdatePlatform,
    install: InstallKind,
    pub(super) status: Status,
    pub(super) release: Option<ReleaseInfo>,
    pub(super) modal_open: bool,
    mock: Option<MockMode>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) worker: Option<worker::Worker>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) staged: Option<Staged>,
}

impl Default for State {
    fn default() -> Self {
        Self::new(env!("CARGO_PKG_VERSION"))
    }
}

impl State {
    /// Creates idle update state for the running `current_version`.
    pub fn new(current_version: &str) -> Self {
        let platform = UpdatePlatform::current();
        #[cfg(not(target_arch = "wasm32"))]
        let install = worker::detect_install(platform);
        #[cfg(target_arch = "wasm32")]
        let install = InstallKind::NotifyOnly;
        Self {
            current_version: current_version.to_owned(),
            platform,
            install,
            status: Status::Idle,
            release: None,
            modal_open: false,
            mock: None,
            #[cfg(not(target_arch = "wasm32"))]
            worker: None,
            #[cfg(not(target_arch = "wasm32"))]
            staged: None,
        }
    }

    pub fn current_version(&self) -> &str {
        &self.current_version
    }
    pub fn platform(&self) -> UpdatePlatform {
        self.platform
    }
    pub fn install(&self) -> &InstallKind {
        &self.install
    }
    pub fn can_self_update(&self) -> bool {
        self.platform != UpdatePlatform::Mac && self.install.can_self_update()
    }
    pub fn status(&self) -> &Status {
        &self.status
    }
    pub fn release(&self) -> Option<&ReleaseInfo> {
        self.release.as_ref()
    }
    pub fn modal_open(&self) -> bool {
        self.modal_open && self.release.is_some()
    }
    pub fn is_mock(&self) -> bool {
        self.mock.is_some()
    }
    pub fn busy(&self) -> bool {
        matches!(
            self.status,
            Status::Checking { .. } | Status::Downloading { .. }
        )
    }

    /// Overrides the active platform (used by developer CLI/option previews).
    pub fn set_platform(&mut self, platform: UpdatePlatform) {
        self.platform = platform;
        if let Some(mode) = self.mock {
            self.install = mock::mock_install(platform, mode);
        } else if platform == UpdatePlatform::Mac {
            self.install = InstallKind::NotifyOnly;
        }
    }

    /// Removes leftover `.old` files and stale staged caches at startup.
    pub fn cleanup_stale(&self, host: &dyn Host) {
        #[cfg(not(target_arch = "wasm32"))]
        worker::cleanup_stale(host.cache_dir().as_deref(), &self.current_version);
        #[cfg(target_arch = "wasm32")]
        let _ = host;
    }

    /// Runs one update action against `settings`, `host`, and `waker`.
    pub fn run(
        &mut self,
        action: Action,
        settings: &UpdateSettings,
        host: &dyn Host,
        waker: &Waker,
    ) -> Outcome {
        match action {
            Action::Check { manual } => self.start_check(manual, settings, waker),
            Action::Install => self.start_install(host, waker),
            Action::Restart => self.restart(host),
            Action::Dismiss => self.dismiss(),
            Action::ShowModal => self.show_modal(),
            Action::OpenDownload => self.open_url(host, true),
            Action::OpenNotes => self.open_url(host, false),
            Action::Mock(spec) => self.apply_mock(spec),
        }
    }

    /// Polls any background check or download job without blocking.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn poll(&mut self, settings: &UpdateSettings, host: &dyn Host, waker: &Waker) -> Outcome {
        self.poll_worker(settings, host, waker)
    }

    /// Polls any background check or download job without blocking.
    #[cfg(target_arch = "wasm32")]
    pub fn poll(&mut self, _s: &UpdateSettings, _h: &dyn Host, _w: &Waker) -> Outcome {
        Outcome::default()
    }

    /// Applies any verified silent staged update cleanly as the app exits.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn apply_on_exit(&mut self, host: &dyn Host) {
        if self.mock.is_some() {
            return;
        }
        if let Some(worker) = self.worker.take() {
            worker.cancel();
        }
        let Some(staged) = self.staged.take() else {
            return;
        };
        let _ = worker::apply_staged(
            &self.install,
            &staged.path,
            &staged.sha256,
            host.state_dir().as_deref(),
            false,
        );
    }

    /// Applies any verified silent staged update cleanly as the app exits.
    #[cfg(target_arch = "wasm32")]
    pub fn apply_on_exit(&mut self, _host: &dyn Host) {}

    fn dismiss(&mut self) -> Outcome {
        let dirty = self.modal_open;
        self.modal_open = false;
        outcome(dirty)
    }

    fn show_modal(&mut self) -> Outcome {
        let dirty = self.release.is_some() && !self.modal_open;
        if self.release.is_some() {
            self.modal_open = true;
        }
        Outcome {
            changed: dirty,
            close_settings: self.release.is_some(),
            ..Outcome::default()
        }
    }

    fn start_check(&mut self, manual: bool, settings: &UpdateSettings, waker: &Waker) -> Outcome {
        if !manual && self.mock.is_some() {
            return Outcome::default();
        }
        if manual && self.mock.is_some() && self.release.is_some() {
            self.modal_open = true;
            return Outcome {
                changed: true,
                close_settings: true,
                ..Outcome::default()
            };
        }
        if (!manual && !should_auto_check(self.platform, &self.install, settings)) || self.busy() {
            return Outcome::default();
        }
        self.spawn_live_check(manual, waker)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_live_check(&mut self, manual: bool, waker: &Waker) -> Outcome {
        self.worker = Some(worker::spawn_check(
            self.current_version.clone(),
            self.platform,
            self.install.clone(),
            manual,
            waker.clone(),
        ));
        self.status = Status::Checking { manual };
        changed()
    }

    #[cfg(target_arch = "wasm32")]
    fn spawn_live_check(&mut self, manual: bool, _waker: &Waker) -> Outcome {
        if manual {
            self.status = Status::Failed("Updates are available in the desktop app.".into());
            return changed();
        }
        Outcome::default()
    }

    fn start_install(&mut self, host: &dyn Host, waker: &Waker) -> Outcome {
        let Some(release) = self.release.clone() else {
            return Outcome::default();
        };
        if !self.can_self_update() {
            return self.open_url(host, true);
        }
        if self.mock.is_some() {
            self.status = Status::ReadyToRestart {
                version: release.version,
                silent: false,
            };
            self.modal_open = true;
            return changed();
        }
        self.spawn_live_install(release, host, waker)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_live_install(
        &mut self,
        release: ReleaseInfo,
        host: &dyn Host,
        waker: &Waker,
    ) -> Outcome {
        if self.staged.is_some() {
            return self.restart(host);
        }
        let Some(cache_dir) = host.cache_dir() else {
            self.status = Status::Failed("No cache directory available for update.".into());
            return changed();
        };
        match worker::spawn_download(release.clone(), cache_dir, false, true, waker.clone()) {
            Ok(job) => {
                self.worker = Some(job);
                self.status = Status::Downloading {
                    received: 0,
                    total: release.asset_size,
                };
            }
            Err(err) => self.status = Status::Failed(err),
        }
        changed()
    }

    #[cfg(target_arch = "wasm32")]
    fn spawn_live_install(&mut self, _r: ReleaseInfo, _h: &dyn Host, _w: &Waker) -> Outcome {
        Outcome::default()
    }

    pub(super) fn restart(&mut self, host: &dyn Host) -> Outcome {
        if self.mock.is_some() {
            let ver = self.release.as_ref().map_or("new", |r| r.version.as_str());
            self.modal_open = false;
            self.status = Status::UpToDate;
            return Outcome {
                changed: true,
                toast: Some(format!(
                    "Mock update applied — Mareader would restart into v{ver}."
                )),
                ..Outcome::default()
            };
        }
        self.apply_live_restart(host)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn apply_live_restart(&mut self, host: &dyn Host) -> Outcome {
        let Some(staged) = self.staged.take() else {
            return Outcome::default();
        };
        match worker::apply_staged(
            &self.install,
            &staged.path,
            &staged.sha256,
            host.state_dir().as_deref(),
            true,
        ) {
            Ok(()) => {
                self.modal_open = false;
                Outcome {
                    changed: true,
                    restart_ready: true,
                    ..Outcome::default()
                }
            }
            Err(err) => {
                self.status = Status::Failed(err);
                self.modal_open = true;
                changed()
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn apply_live_restart(&mut self, _host: &dyn Host) -> Outcome {
        Outcome::default()
    }

    fn open_url(&mut self, host: &dyn Host, download: bool) -> Outcome {
        let Some(release) = self.release.as_ref() else {
            return Outcome::default();
        };
        let url = if download {
            &release.download_url
        } else {
            &release.html_url
        };
        if !validate_https_url(url) {
            return Outcome::default();
        }
        if !host.open_external(std::path::Path::new(url)) {
            self.status = Status::Failed("Could not open link in the system browser.".into());
            return changed();
        }
        if download && !self.can_self_update() {
            self.modal_open = false;
        }
        changed()
    }

    fn apply_mock(&mut self, spec: MockUpdate) -> Outcome {
        let built = mock::build(spec, self.platform);
        self.platform = built.platform;
        self.install = built.install;
        self.mock = Some(built.mode);
        self.release = Some(built.release);
        self.status = built.status;
        self.modal_open = built.modal_open;
        changed()
    }
}

pub(super) fn changed() -> Outcome {
    outcome(true)
}

fn outcome(changed: bool) -> Outcome {
    Outcome {
        changed,
        ..Outcome::default()
    }
}
