//! The dock's cards: the run id every progress beat echoes, and the lifecycle of the card
//! that reports the run. Written from the import modules rather than from the dock: a view
//! that owned the lifecycle of the thing it renders would have to outlive the import it is
//! reporting on.

use std::sync::atomic::{AtomicU32, Ordering};

use leptos::prelude::*;

use crate::services::library::toast;
use crate::state::library::ImportTask;
use crate::state::AppState;
use crate::time::now_ms;

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

pub fn dismiss_task(state: AppState, id: &str) {
    let id = id.to_string();
    state
        .library
        .tasks
        .update(|tasks| tasks.retain(|t| t.id != id));
}

/// One spelling for the runs that finish with one — a folder walk, a loose-file drop and a restore.
pub(super) fn finish_task(state: AppState, task: &str, total: u32, waiting: u32) {
    update_task(state, task, move |t| {
        t.total = total;
        t.done = total;
        t.waiting = waiting;
        t.finish();
    });
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
