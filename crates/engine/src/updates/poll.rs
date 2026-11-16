//! Non-blocking worker message handling for desktop update jobs.

use mareader_settings::UpdateSettings;

use crate::host::Host;
use crate::session::Waker;

use super::state::{Staged, State, changed};
use super::worker;
use super::{Outcome, ReleaseInfo, Status};

impl State {
    pub(super) fn poll_worker(
        &mut self,
        settings: &UpdateSettings,
        host: &dyn Host,
        waker: &Waker,
    ) -> Outcome {
        let mut msgs = Vec::new();
        let mut done = false;
        if let Some(worker) = self.worker.as_ref() {
            loop {
                match worker.rx.try_recv() {
                    Ok(msg) => msgs.push(msg),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        done = true;
                        break;
                    }
                }
            }
        }
        if done {
            self.worker = None;
        }
        let mut out = Outcome::default();
        for msg in msgs {
            let step = self.handle_msg(msg, settings, host, waker);
            out.changed |= step.changed;
            out.close_settings |= step.close_settings;
            out.pending_restart |= step.pending_restart;
            out.restart_ready |= step.restart_ready;
            out.toast = step.toast.or(out.toast);
        }
        out
    }

    fn handle_msg(
        &mut self,
        msg: worker::Msg,
        settings: &UpdateSettings,
        host: &dyn Host,
        waker: &Waker,
    ) -> Outcome {
        match msg {
            worker::Msg::Checked { manual, result } => {
                self.worker = None;
                self.on_checked(manual, result, settings, host, waker)
            }
            worker::Msg::Progress { received, total } => {
                self.status = Status::Downloading { received, total };
                changed()
            }
            worker::Msg::Staged {
                version,
                path,
                sha256,
                silent,
                restart_after,
            } => {
                self.worker = None;
                self.staged = Some(Staged { path, sha256 });
                self.status = Status::ReadyToRestart { version, silent };
                Outcome {
                    changed: true,
                    pending_restart: restart_after,
                    ..Outcome::default()
                }
            }
            worker::Msg::DownloadFailed { silent, error } => {
                self.worker = None;
                self.status = Status::Failed(error);
                if silent {
                    self.modal_open = false;
                }
                changed()
            }
        }
    }

    fn on_checked(
        &mut self,
        manual: bool,
        result: Result<Option<ReleaseInfo>, String>,
        settings: &UpdateSettings,
        host: &dyn Host,
        waker: &Waker,
    ) -> Outcome {
        match result {
            Ok(Some(info)) => {
                let can_silent = !manual
                    && settings.auto_update
                    && self.can_self_update()
                    && info.asset_url.is_some()
                    && info.sha256.is_some();
                self.release = Some(info.clone());
                if can_silent {
                    self.start_silent_download(info, host, waker);
                    return changed();
                }
                self.status = Status::Available;
                self.modal_open = true;
                Outcome {
                    changed: true,
                    close_settings: manual,
                    ..Outcome::default()
                }
            }
            Ok(None) => {
                self.release = None;
                self.status = if manual {
                    Status::UpToDate
                } else {
                    Status::Idle
                };
                changed()
            }
            Err(err) => {
                self.status = Status::Failed(err);
                if !manual {
                    self.modal_open = false;
                }
                changed()
            }
        }
    }

    fn start_silent_download(&mut self, info: ReleaseInfo, host: &dyn Host, waker: &Waker) {
        self.modal_open = false;
        let Some(cache_dir) = host.cache_dir() else {
            self.status = Status::Available;
            return;
        };
        match worker::spawn_download(info.clone(), cache_dir, true, false, waker.clone()) {
            Ok(job) => {
                self.worker = Some(job);
                self.status = Status::Downloading {
                    received: 0,
                    total: info.asset_size,
                };
            }
            Err(_) => self.status = Status::Available,
        }
    }
}
