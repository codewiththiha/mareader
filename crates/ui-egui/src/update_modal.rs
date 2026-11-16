//! Centered in-app startup update modal showing structured release highlights.

use egui::{Context, Frame, Label, Margin, ProgressBar, RichText, ScrollArea, Stroke, Ui};
use mareader_engine::command::Command;
use mareader_engine::updates::{Action, ReleaseInfo, State, Status, UpdatePlatform};

use crate::icons::Icon;
use crate::modal::{self, Dialog};
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};

/// Draws the startup/manual update modal when an update prompt is open.
pub(crate) fn show(ctx: &Context, updates: &State, out: &mut Commands) {
    if !updates.modal_open() {
        return;
    }
    let Some(release) = updates.release() else {
        return;
    };
    let t = Tokens::get(ctx);
    let width = (ctx.content_rect().width() - 48.0).clamp(280.0, 460.0);
    let subtitle = format!(
        "Mareader {} is available (you have {}).",
        release.version,
        updates.current_version()
    );

    let modal = Dialog::new("update-available").show(ctx, |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 12.0;

        modal::heading(
            ui,
            Some((Icon::Download, t.accent)),
            "Update available",
            Some(&subtitle),
            &t,
        );

        summary_card(ui, release, &t);
        status_band(ui, updates, &t);
        action_row(ui, updates, release, &t, out);
    });

    if modal.should_close() {
        out.push(Sent::Engine(Command::Update(Action::Dismiss)));
    }
}

fn summary_card(ui: &mut Ui, release: &ReleaseInfo, t: &Tokens) {
    Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius)
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ScrollArea::vertical()
                .id_salt(("update-summary", &release.tag))
                .max_height(240.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    if release.summary.is_empty() {
                        let text = "See the GitHub release page for what changed in this version.";
                        ui.add(Label::new(RichText::new(text).color(t.muted)).wrap());
                        return;
                    }
                    if !release.summary.added.is_empty() {
                        bullet_group(ui, "New", &release.summary.added, t);
                    }
                    if !release.summary.added.is_empty() && !release.summary.fixes.is_empty() {
                        ui.add_space(6.0);
                    }
                    if !release.summary.fixes.is_empty() {
                        bullet_group(ui, "Fixes", &release.summary.fixes, t);
                    }
                });
        });
}

fn bullet_group(ui: &mut Ui, title: &str, items: &[String], t: &Tokens) {
    ui.label(
        RichText::new(title)
            .font(theme::medium(t.text_small))
            .color(t.muted),
    );
    for item in items {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new("•").color(t.accent));
            ui.add(Label::new(RichText::new(item).color(t.ink)).wrap());
        });
    }
}

fn status_band(ui: &mut Ui, updates: &State, t: &Tokens) {
    match updates.status() {
        Status::Downloading { received, total } => {
            let (frac, label) = match total {
                Some(total) if *total > 0 => {
                    let ratio = (*received as f32 / *total as f32).clamp(0.0, 1.0);
                    (ratio, format!("Downloading… {:.0}%", ratio * 100.0))
                }
                _ => (0.0, format!("Downloading… {} KB", received / 1024)),
            };
            ui.add(
                ProgressBar::new(frac)
                    .fill(t.accent)
                    .text(RichText::new(label).size(t.text_small)),
            );
        }
        Status::ReadyToRestart { version, .. } => {
            let msg = format!("Version {version} is verified and ready to install.");
            ui.add(Label::new(RichText::new(msg).size(t.text_small).color(t.muted)).wrap());
        }
        Status::Failed(err) => {
            ui.add(Label::new(RichText::new(err).size(t.text_small).color(t.danger)).wrap());
        }
        _ if !updates.can_self_update() => {
            let hint = match updates.platform() {
                UpdatePlatform::Mac => {
                    "Opens the download in your browser. Drag the updated app into Applications."
                }
                UpdatePlatform::Windows | UpdatePlatform::Linux => {
                    "Installed in a system folder — download the release or update via your package manager."
                }
            };
            ui.add(Label::new(RichText::new(hint).size(t.text_small).color(t.muted)).wrap());
        }
        _ => {}
    }
}

fn action_row(ui: &mut Ui, updates: &State, release: &ReleaseInfo, t: &Tokens, out: &mut Commands) {
    modal::buttons(ui, |ui| match updates.status() {
        Status::Downloading { .. } => {
            if modal::quiet(ui, "Hide", t).clicked() {
                out.push(Sent::Engine(Command::Update(Action::Dismiss)));
            }
        }
        Status::ReadyToRestart { .. } => {
            if modal::primary(ui, "Restart now", t.accent, t).clicked() {
                out.push(Sent::Engine(Command::Update(Action::Restart)));
            }
            if modal::quiet(ui, "Later", t).clicked() {
                out.push(Sent::Engine(Command::Update(Action::Dismiss)));
            }
        }
        Status::Failed(_) => {
            if modal::primary(ui, "Download manually", t.accent, t).clicked() {
                out.push(Sent::Engine(Command::Update(Action::OpenDownload)));
            }
            if modal::quiet(ui, "Later", t).clicked() {
                out.push(Sent::Engine(Command::Update(Action::Dismiss)));
            }
        }
        _ if updates.can_self_update() => {
            if modal::primary(ui, "Update & Restart", t.accent, t).clicked() {
                out.push(Sent::Engine(Command::Update(Action::Install)));
            }
            if modal::quiet(ui, "Release notes", t).clicked() {
                out.push(Sent::Engine(Command::Update(Action::OpenNotes)));
            }
            if modal::quiet(ui, "Later", t).clicked() {
                out.push(Sent::Engine(Command::Update(Action::Dismiss)));
            }
        }
        _ => {
            if modal::primary(ui, "Download update", t.accent, t).clicked() {
                out.push(Sent::Engine(Command::Update(Action::OpenDownload)));
            }
            if release.download_url != release.html_url
                && modal::quiet(ui, "Release notes", t).clicked()
            {
                out.push(Sent::Engine(Command::Update(Action::OpenNotes)));
            }
            if modal::quiet(ui, "Later", t).clicked() {
                out.push(Sent::Engine(Command::Update(Action::Dismiss)));
            }
        }
    });
}
