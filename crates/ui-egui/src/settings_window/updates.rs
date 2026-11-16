//! Settings › Updates: startup checks, silent auto-update, and manual check.

use egui::{Button, RichText, Stroke, Ui, vec2};
use mareader_engine::command::Command;
use mareader_engine::updates::{Action, State, Status, UpdatePlatform};
use mareader_settings::UpdateSettings;

use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

use super::{detail_row, divider, group, note, section, switch_row};

/// Draws the Updates settings tab for the active platform.
pub(super) fn tab(ui: &mut Ui, s: &mut UpdateSettings, updates: &State, out: &mut Commands) {
    let platform = updates.platform();
    section(ui, "Updates");
    group(ui, |ui| match platform {
        UpdatePlatform::Mac => {
            switch_row(
                ui,
                "Notify about updates",
                "Check GitHub Releases at startup and show a download prompt when newer",
                &mut s.check_on_startup,
            );
        }
        UpdatePlatform::Windows | UpdatePlatform::Linux => {
            switch_row(
                ui,
                "Auto-update",
                "Download and install updates silently in the background without interrupting you",
                &mut s.auto_update,
            );
            divider(ui);
            switch_row(
                ui,
                "Check for updates automatically",
                "Check at startup and open the update prompt when Auto-update is off",
                &mut s.check_on_startup,
            );
        }
    });

    match platform {
        UpdatePlatform::Mac => {
            note(
                ui,
                "On macOS, updates open the release download link in your browser.",
            );
        }
        UpdatePlatform::Windows | UpdatePlatform::Linux if !updates.can_self_update() => {
            note(
                ui,
                "Installed in a system or package-managed location — Mareader notifies you with a download link instead of modifying system files.",
            );
        }
        UpdatePlatform::Windows | UpdatePlatform::Linux if s.auto_update => {
            note(
                ui,
                "Updates verify their SHA-256 checksum in the background and install cleanly when you close Mareader.",
            );
        }
        UpdatePlatform::Windows | UpdatePlatform::Linux => {}
    }

    version_section(ui, updates, out);
}

fn version_section(ui: &mut Ui, updates: &State, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let title = format!("Mareader {}", updates.current_version());
    let status = status_text(updates);
    let busy = updates.busy();

    section(ui, "Version");
    group(ui, |ui| {
        detail_row(ui, &title, &status, |ui| {
            let label = if matches!(updates.status(), Status::Checking { .. }) {
                "Checking…"
            } else {
                "Check for updates"
            };
            let fg = if busy { t.muted } else { t.accent };
            let bg = if busy { t.surface } else { t.accent_soft };
            let btn = Button::new(RichText::new(label).color(fg))
                .fill(bg)
                .corner_radius(t.radius)
                .min_size(vec2(124.0, 28.0));
            if ui.add_enabled(!busy, btn).clicked() {
                out.push(Sent::Engine(Command::Update(Action::Check {
                    manual: true,
                })));
            }
        });

        if let Status::ReadyToRestart { version, .. } = updates.status() {
            divider(ui);
            let head = format!("Version {version} ready");
            let sub = "Verified and staged — installs on exit, or restart now";
            detail_row(ui, &head, sub, |ui| {
                let btn = widgets::primary_button(ui, None, "Restart to update", t.accent, &t)
                    .min_size(vec2(124.0, 28.0));
                if ui.add(btn).clicked() {
                    out.push(Sent::Engine(Command::Update(Action::Restart)));
                }
            });
        } else if let Some(release) = updates.release() {
            divider(ui);
            let head = format!("Version {} available", release.version);
            let sub = "Review release highlights and download options";
            detail_row(ui, &head, sub, |ui| {
                let btn = Button::new(RichText::new("View update").color(t.ink))
                    .fill(t.surface)
                    .stroke(Stroke::new(1.0, t.line))
                    .corner_radius(t.radius)
                    .min_size(vec2(100.0, 28.0));
                if ui.add(btn).clicked() {
                    out.push(Sent::Engine(Command::Update(Action::ShowModal)));
                }
            });
        }
    });
}

fn status_text(updates: &State) -> String {
    match updates.status() {
        Status::Idle => "Ready to check GitHub Releases".to_owned(),
        Status::Checking { .. } => "Checking GitHub Releases…".to_owned(),
        Status::UpToDate => "You are on the latest version".to_owned(),
        Status::Available => match updates.release() {
            Some(r) => format!("Version {} is available", r.version),
            None => "An update is available".to_owned(),
        },
        Status::Downloading { received, total } => match total {
            Some(t) if *t > 0 => {
                let pct = ((*received as f64 / *t as f64) * 100.0).clamp(0.0, 100.0);
                format!("Downloading update… {pct:.0}%")
            }
            _ => format!("Downloading update… {} KB", received / 1024),
        },
        Status::ReadyToRestart { version, silent } => {
            if *silent {
                format!("Version {version} ready — will install on exit")
            } else {
                format!("Version {version} ready to install")
            }
        }
        Status::Failed(err) => err.clone(),
    }
}
