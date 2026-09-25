//! One walk per root: the synchronous claim that keeps two runs off one folder's ledger,
//! and the drop guard that releases it however a run ends.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::services::library::{folder_label, toast};
use crate::state::AppState;

use super::Asked;

thread_local! {
    /// One run per root, claimed synchronously and released when the run's future drops — see [`claim_root`].
    static RUNNING: RefCell<HashMap<String, Asked>> = RefCell::new(HashMap::new());
    /// One start per root, run by the rescan's own release rather than polled
    /// for, so an ask is never refused by a walk the app started for itself.
    static WAITING: RefCell<HashMap<String, Box<dyn FnOnce()>>> = RefCell::new(HashMap::new());
}

pub(super) struct RootClaim(String);

impl Drop for RootClaim {
    fn drop(&mut self) {
        RUNNING.with(|running| running.borrow_mut().remove(&self.0));
        // Taken out and called with no borrow outstanding: the start it
        // hands over claims this very root.
        let start = WAITING.with(|waiting| waiting.borrow_mut().remove(&self.0));
        if let Some(start) = start {
            start();
        }
    }
}

/// The sentence a second ask for a walking folder gets. One spelling: the
/// two doors that can refuse a run refuse it for the same reason.
pub(super) fn already_importing(state: AppState, root: &str) {
    toast(
        state,
        format!("{} is already being imported.", folder_label(root)),
    );
}

fn holder(root: &str) -> Option<Asked> {
    RUNNING.with(|running| running.borrow().get(root).copied())
}

pub(super) fn root_is_claimed(root: &str) -> bool {
    holder(root).is_some() || WAITING.with(|waiting| waiting.borrow().contains_key(root))
}

/// Two concurrent walks of one folder are two snapshots of the same ledger
/// row and two writes back; the second write drops whatever the first placed.
/// Hence the check-and-claim is one write.
pub(super) fn claim_root(root: &str, asked: Asked) -> Option<RootClaim> {
    let free = RUNNING.with(|running| {
        running
            .borrow_mut()
            .insert(root.to_string(), asked)
            .is_none()
    });
    free.then(|| RootClaim(root.to_string()))
}

/// Runs `start` when the root is free, queues it behind a focus rescan, and
/// answers `false` when the root belongs to an ask the reader made — the
/// refusal the caller owes a sentence for. An ask outranks a rescan.
pub(super) fn when_root_is_free<F>(root: &str, start: F) -> bool
where
    F: FnOnce() + 'static,
{
    match holder(root) {
        None => {
            start();
            true
        }
        Some(Asked::OnFocus) => {
            WAITING.with(
                |waiting| match waiting.borrow_mut().entry(root.to_string()) {
                    Entry::Occupied(_) => false,
                    Entry::Vacant(slot) => {
                        slot.insert(Box::new(start));
                        true
                    }
                },
            )
        }
        Some(Asked::Explicitly) => false,
    }
}

/// The guarded start the run doors share — a folder import, a copies run:
/// the claim, the card and the double-import sentence in one place, so the
/// ordering (run starts inside the claim, card goes up only when there is a
/// run) is not copy-paste folklore at each door.
///
/// `launch` receives the task id its beats echo and the root it walks; a root
/// held by a rescan queues the launch for the walk's release
/// ([`when_root_is_free`]'s rule).
pub(super) fn start_guarded(
    state: AppState,
    root: &str,
    launch: impl FnOnce(AppState, String, String) + 'static,
) {
    let task = super::tasks::task_id();
    let card = task.clone();
    let walking = root.to_string();
    if !when_root_is_free(root, move || launch(state, task, walking)) {
        already_importing(state, root);
        return;
    }
    super::tasks::push_task(
        state,
        crate::state::library::ImportTask::new(card, folder_label(root)),
    );
}

/// The claim gate without a card, for doors whose run and card come from
/// deeper in (a replace handing its root to `proceed_folder`): the same
/// refusal sentence, no second card for one run.
pub(super) fn gate_root(state: AppState, root: &str, launch: impl FnOnce() + 'static) {
    if !when_root_is_free(root, launch) {
        already_importing(state, root);
    }
}
