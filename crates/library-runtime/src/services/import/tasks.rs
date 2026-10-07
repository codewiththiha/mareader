//! The dock's cards: the run id and the card's lifecycle.

use leptos::prelude::*;

use crate::services::toast;
use crate::state::library::ImportTask;
use runtime_contract::time::now_ms;

/// Minted by the library's own counter, like every other id.
pub(super) fn task_id() -> String {
    library_core::id::next_task_id(now_ms())
}

pub(super) fn push_task(state: crate::context::LibraryContext, task: ImportTask) {
    state.library.tasks.update(|tasks| tasks.push(task));
}

/// Mint the id and put the card up for a run that ends itself.
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

/// One spelling of "finished" for runs that end with a count.
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

/// The expected total and the finish's, so the card cannot end past 100%.
pub(super) fn run_total(landed: u32, reconciled: usize, represented: usize) -> u32 {
    landed + (reconciled + represented) as u32
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FailMode {
    Toast,
    /// A folder that cannot be read fails again on the next focus.
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
