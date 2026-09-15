//! The dock's cards: the run id every progress beat echoes, and the lifecycle of the card
//! that reports the run. Written from the import modules rather than from the dock: a view
//! that owned the lifecycle of the thing it renders would have to outlive the import it is
//! reporting on.

use leptos::prelude::*;

use crate::services::library::toast;
use crate::state::library::ImportTask;
use crate::state::AppState;
use crate::time::now_ms;

/// Minted by [`library_core::id`]'s own counter, like every other id the library hands out:
/// two runs minted in one millisecond never share a card, and the rule is tested where the
/// rest of the id scheme is.
pub(super) fn task_id() -> String {
    library_core::id::next_task_id(now_ms())
}

pub(super) fn push_task(state: AppState, task: ImportTask) {
    state.library.tasks.update(|tasks| tasks.push(task));
}

/// A run that will report its own end: mint the id, put the card up. The single-book copies
/// (a relink, a duplicate, a landing an answer owed, a replace's conversion) go through here
/// rather than minting a task id nothing subscribes to — a beat for an id the dock does not
/// hold is dropped, and the shell's throttled emissions were wasted IPC telling nobody
/// anything.
pub(crate) fn begin_task(state: AppState, label: impl Into<String>) -> String {
    let task = task_id();
    push_task(state, ImportTask::new(task.clone(), label));
    task
}

pub(super) fn update_task(state: AppState, id: &str, change: impl FnOnce(&mut ImportTask) + 'static) {
    let id = id.to_string();
    state.library.tasks.update(|tasks| {
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            change(task);
        }
    });
}

pub fn dismiss_task(state: AppState, id: &str) {
    let id = id.to_string();
    state
        .library
        .tasks
        .update(|tasks| tasks.retain(|t| t.id != id));
}

/// One spelling for the runs that finish with one — a folder walk, a loose-file drop and a restore.
pub(crate) fn finish_task(state: AppState, task: &str, total: u32, waiting: u32) {
    update_task(state, task, move |t| {
        t.total = total;
        t.done = total;
        t.waiting = waiting;
        t.finish();
    });
}

/// A single-copy run's failure: the card carries the sentence, and the reader hears it too.
pub(crate) fn fail_task(state: AppState, task: &str, message: String) {
    fail(state, task, message, FailMode::Toast);
}

/// One arithmetic for the two counts a run reports — the expected total the card opens with
/// and the finish's — because the two were two writers that happened to agree: `run_folder`
/// counted its expected WITHOUT the represented rows (they read as already done) and its
/// finish WITH them, and only the coincidence of the two sums kept the card from ending
/// past 100%. Both moments now ask here.
///
/// `represented` are rows a log named as already held: they count at the finish, where the
/// reader is told what the run lit up, and not at the expected, where they would promise
/// copies that were never going to be made.
pub(super) fn run_total(landed: u32, reconciled: usize, represented: usize) -> u32 {
    landed + (reconciled + represented) as u32
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FailMode {
    Toast,
    /// A folder that cannot be read is not news the reader asked for, and it fails again on the next focus.
    ConsoleOnly,
}

pub(super) fn fail(state: AppState, task: &str, message: String, mode: FailMode) {
    if mode == FailMode::ConsoleOnly {
        web_sys::console::warn_1(&format!("[library] rescan failed: {message}").into());
        return;
    }
    let sentence = message.clone();
    update_task(state, task, move |t| t.fail(message));
    toast(state, sentence);
}

#[cfg(test)]
mod tests {
    use super::run_total;

    #[test]
    fn the_expected_count_and_the_finish_count_agree_on_one_arithmetic() {
        // The card's opening promise and its closing report are the same sum, so a run
        // cannot finish past the total it opened with.
        assert_eq!(run_total(3, 2, 0), 5);
        assert_eq!(run_total(3, 2, 4), 9);
        assert_eq!(run_total(0, 0, 7), 7);
    }
}
