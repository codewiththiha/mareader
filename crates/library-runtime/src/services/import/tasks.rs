//! The dock's cards: the run id every progress beat echoes, and the lifecycle
//! of the card that reports the run. Written from the import modules, not the
//! dock: a view that owned the lifecycle of what it renders would have to
//! outlive the import it reports on.

use leptos::prelude::*;

use crate::services::toast;
use crate::state::library::ImportTask;
use runtime_contract::time::now_ms;

/// Minted by [`library_core::id`]'s own counter, like every other library id:
/// two runs minted in one millisecond never share a card.
pub(super) fn task_id() -> String {
    library_core::id::next_task_id(now_ms())
}

pub(super) fn push_task(state: crate::context::LibraryContext, task: ImportTask) {
    state.library.tasks.update(|tasks| tasks.push(task));
}

/// Mint the id and put the card up for a run that will report its own end.
/// Single-book copies (a relink, a duplicate, a replace's conversion) go
/// through here: a beat for an id the dock does not hold is dropped, and the
/// shell's emissions would be wasted IPC telling nobody anything.
pub(crate) fn begin_task(
    state: crate::context::LibraryContext,
    label: impl Into<String>,
) -> String {
    let task = task_id();
    push_task(state, ImportTask::new(task.clone(), label));
    task
}

pub(super) fn update_task(
    state: crate::context::LibraryContext,
    id: &str,
    change: impl FnOnce(&mut ImportTask) + 'static,
) {
    let id = id.to_string();
    state.library.tasks.update(|tasks| {
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            change(task);
        }
    });
}

pub fn dismiss_task(state: crate::context::LibraryContext, id: &str) {
    let id = id.to_string();
    state
        .library
        .tasks
        .update(|tasks| tasks.retain(|t| t.id != id));
}

/// One spelling of "finished" for the runs that end with a count — a folder
/// walk, a loose-file drop and a restore.
pub(crate) fn finish_task(
    state: crate::context::LibraryContext,
    task: &str,
    total: u32,
    waiting: u32,
) {
    update_task(state, task, move |t| {
        t.total = total;
        t.done = total;
        t.waiting = waiting;
        t.finish();
    });
}

/// A single-copy run's failure: the card carries the sentence and the reader
/// hears it too.
pub(crate) fn fail_task(state: crate::context::LibraryContext, task: &str, message: String) {
    fail(state, task, message, FailMode::Toast);
}

/// One arithmetic for the two counts a run reports — the expected total the
/// card opens with and the finish's — so the card cannot end past 100%.
///
/// `represented` rows (a log named them as already held) count at the finish,
/// where the reader is told what the run lit up, and not in the expected
/// total, where they would promise copies that were never going to be made.
pub(super) fn run_total(landed: u32, reconciled: usize, represented: usize) -> u32 {
    landed + (reconciled + represented) as u32
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FailMode {
    Toast,
    /// A folder that cannot be read is not news the reader asked for, and it
    /// fails again on the next focus.
    ConsoleOnly,
}

pub(super) fn fail(
    state: crate::context::LibraryContext,
    task: &str,
    message: String,
    mode: FailMode,
) {
    if mode == FailMode::ConsoleOnly {
        web_sys::console::warn_1(&format!("[library] rescan failed: {message}").into());
        return;
    }
    let sentence = message.clone();
    update_task(state, task, move |t| t.fail(message));
    toast(state, sentence);
}
