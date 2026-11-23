//! What each saved model runs and queues now, drawn in egui under its row.

use std::time::Duration;

use egui::{RichText, Ui};
use mareader_assist::JobInfo;
use mareader_settings::PresetId;

use crate::theme::Tokens;

/// Where a job's line starts: past the row's badge, under its name.
const INDENT: f32 = 42.0;

/// Tasks `id` runs now.
pub(super) fn running(jobs: &[JobInfo], id: PresetId) -> usize {
    jobs.iter().filter(|j| j.preset == id && j.running).count()
}

/// A line per task on `id`, those running first; nothing when idle.
pub(super) fn list(ui: &mut Ui, jobs: &[JobInfo], id: PresetId) {
    let mine = jobs.iter().filter(|j| j.preset == id);
    let t = Tokens::get(ui.ctx());
    let mut any = false;
    for job in mine {
        any = true;
        let color = if job.running { t.ink } else { t.muted };
        let line = RichText::new(said(job, job.since.elapsed()));
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            ui.label(line.size(t.text_small).color(color));
        });
    }
    if any {
        // The seconds tick on while anything runs or waits.
        ui.add_space(6.0);
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

/// "AI Dictionary · running 4 s", for a job `took` old.
fn said(job: &JobInfo, took: Duration) -> String {
    let doing = if job.running { "running" } else { "waiting" };
    let secs = took.as_secs();
    let took = match secs {
        0..60 => format!("{secs} s"),
        _ => format!("{} min", secs / 60),
    };
    format!("{} · {doing} {took}", job.task.spec().label)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    use mareader_settings::TaskId;

    use super::*;

    fn job(task: TaskId, preset: u32, running: bool) -> JobInfo {
        JobInfo {
            task,
            preset: PresetId(preset),
            model: "Fast".to_owned(),
            running,
            since: Instant::now(),
        }
    }

    #[test]
    fn a_line_names_the_task_what_it_does_and_for_how_long() {
        let explain = job(TaskId::Explain, 1, true);
        let cards = job(TaskId::StudyCards, 1, false);
        let secs = Duration::from_secs;
        assert_eq!(said(&explain, secs(4)), "AI Dictionary · running 4 s");
        assert_eq!(said(&cards, secs(150)), "Study cards · waiting 2 min");
    }

    #[test]
    fn each_model_lists_only_its_own_jobs() {
        let jobs = vec![
            job(TaskId::Organize, 1, true),
            job(TaskId::Explain, 1, false),
            job(TaskId::Summarize, 2, true),
        ];
        assert_eq!(running(&jobs, PresetId(1)), 1);
        assert_eq!(running(&jobs, PresetId(3)), 0);
        let harness = Harness::new_ui(|ui| list(ui, &jobs, PresetId(1)));
        harness.get_by_label("Organize library · running 0 s");
        harness.get_by_label("AI Dictionary · waiting 0 s");
        assert!(harness.query_by_label("Summarize · running 0 s").is_none());
    }
}
