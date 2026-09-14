//! The dock's cards: the run id every progress beat echoes, and the
//! lifecycle of the card that reports the run — pushed when a walk starts,
//! beat by the shell's progress sink, closed on the run's final counts.
//! Written from the import modules rather than from the dock: the dock is a
//! view, and a view that owned the lifecycle of the thing it renders would
//! have to outlive the import it is reporting on.

use std::sync::atomic::{AtomicU32, Ordering};

use leptos::prelude::*;

use crate::services::library::toast;
use crate::state::library::ImportTask;
use crate::state::AppState;
use crate::time::now_ms;

/// A run's id. The shell echoes it on every progress beat, so two imports in
/// flight never mix their counts, and the dock can look a card up by it.
pub(super) fn task_id() -> String {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    format!(
        "t{:x}-{}",
        now_ms(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

pub(super) fn push_task(state: AppState, task: ImportTask) {
    state.library.tasks.update(|tasks| tasks.push(task));
}

pub(super) fn update_task(state: AppState, id: &str, change: impl FnOnce(&mut ImportTask) + 'static) {
    let id = id.to_string();
    state.library.tasks.update(|tasks| {
        if let Some(task) = tasks.iter_mut().find(|t| t.id == id) {
            change(task);
        }
    });
}

/// Take a card out of the dock. The dock asks for this on a timer of its own;
/// nothing here decides how long a reader gets to look at a finished import.
pub fn dismiss_task(state: AppState, id: &str) {
    let id = id.to_string();
    state
        .library
        .tasks
        .update(|tasks| tasks.retain(|t| t.id != id));
}

/// Close a dock card on its final counts.
///
/// One spelling for the runs that finish one — a folder walk, a loose-file drop
/// and a restore — because a card is a report and three routes into the library
/// reporting three different sets of numbers is three answers about one import.
pub(super) fn finish_task(state: AppState, task: &str, total: u32, waiting: u32) {
    update_task(state, task, move |t| {
        t.total = total;
        t.done = total;
        t.waiting = waiting;
        t.finish();
    });
}

/// How a run's failure is delivered.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FailMode {
    /// A run the reader asked for: the card shows it and a toast says it.
    Toast,
    /// The window-focus rescan: one console line, where a bug report can find
    /// it. A folder that cannot be read is not news the reader asked for, and
    /// it fails again on the next focus.
    ConsoleOnly,
}

pub(super) fn fail(state: AppState, task: &str, message: String, mode: FailMode) {
    if mode == FailMode::ConsoleOnly {
        // A watched folder that cannot be read is not news the reader asked
        // for, and it fails again on the next focus. Say it once, on the
        // console, where a bug report can find it.
        web_sys::console::warn_1(&format!("[library] rescan failed: {message}").into());
        return;
    }
    let sentence = message.clone();
    update_task(state, task, move |t| t.fail(message));
    toast(state, sentence);
}
