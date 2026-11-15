//! A download's controls in its row: word data, pronunciation, voice models.

use egui::{ProgressBar, RichText, Ui};
use mareader_speech::Stage;

use crate::dict::megabytes;
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::dict::Download;

/// One download's buttons: labelled "{verb} {name}", each asking `ask`.
pub(super) struct Buttons<'a, F: Fn(Download) -> Sent> {
    pub name: &'a str,
    pub ask: F,
}

impl<F: Fn(Download) -> Sent> Buttons<'_, F> {
    pub fn press(
        &self,
        ui: &mut Ui,
        (icon, verb): (Icon, &str),
        what: Download,
        out: &mut Commands,
    ) {
        let label = format!("{verb} {}", self.name);
        if widgets::icon_button(ui, icon, &label, false).clicked() {
            out.push((self.ask)(what));
        }
    }

    /// The remove button, in the danger colour.
    pub fn remove(&self, ui: &mut Ui, out: &mut Commands) {
        let danger = Tokens::get(ui.ctx()).danger;
        ui.scope(|ui| {
            ui.visuals_mut().override_text_color = Some(danger);
            self.press(ui, (Icon::Remove, "Remove"), Download::Remove, out);
        });
    }

    pub fn paused(&self, ui: &mut Ui, got: u64, out: &mut Commands) {
        self.press(ui, (Icon::Close, "Cancel"), Download::Cancel, out);
        self.press(ui, (Icon::Resume, "Resume"), Download::Start, out);
        muted(ui, &format!("Paused at {}", megabytes(got)));
    }

    /// Another try, and why the last one failed on hover.
    pub fn failed(&self, ui: &mut Ui, why: &str, out: &mut Commands) {
        self.press(ui, (Icon::Rescan, "Try again"), Download::Start, out);
        let danger = Tokens::get(ui.ctx()).danger;
        let r = ui.label(RichText::new("Couldn't download").color(danger));
        widgets::tip(&r, why, None);
    }

    /// Buttons and progress for `stage`, right to left; `bytes` is its size.
    pub fn controls(&self, ui: &mut Ui, stage: &Stage, bytes: u64, out: &mut Commands) {
        match stage {
            Stage::Absent => {
                self.press(ui, (Icon::Download, "Download"), Download::Start, out);
                muted(ui, &megabytes(bytes));
            }
            Stage::Ready => {
                muted(ui, "Downloaded");
            }
            Stage::Waiting => {
                self.press(ui, (Icon::Close, "Cancel"), Download::Cancel, out);
                muted(ui, "Waiting…");
            }
            Stage::Fetching { got, total } => {
                self.press(ui, (Icon::Close, "Cancel"), Download::Cancel, out);
                self.press(ui, (Icon::Pause, "Pause"), Download::Pause, out);
                progress(ui, *got, *total);
            }
            Stage::Paused { got, .. } => self.paused(ui, *got, out),
            Stage::Checking => {
                ui.spinner();
                muted(ui, "Checking…");
            }
            Stage::Failed(why) => self.failed(ui, why, out),
        }
    }
}

/// How much of `total` bytes is in, as a bar.
pub(super) fn progress(ui: &mut Ui, got: u64, total: u64) {
    let total = total.max(1);
    let done = (got as f32 / total as f32).clamp(0.0, 1.0);
    let text = format!("{} of {}", megabytes(got), megabytes(total));
    ui.add(ProgressBar::new(done).desired_width(150.0).text(text));
}

/// Small grey text beside a row's controls.
pub(super) fn muted(ui: &mut Ui, text: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).size(t.text_small).color(t.muted))
}
