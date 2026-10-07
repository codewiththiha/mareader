//! One walk per root: the claim, and the guard that releases it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::context::LibraryContext;
use crate::services::{folder_label, toast};

use super::Asked;

thread_local! {
    /// One run per root, claimed synchronously; [`claim_root`].
    static RUNNING: RefCell<HashMap<String, Asked>> = RefCell::new(HashMap::new());
    /// One start per root, run at the rescan's release, not polled for.
    static WAITING: RefCell<HashMap<String, Box<dyn FnOnce()>>> = RefCell::new(HashMap::new());
}

pub(super) struct RootClaim(String);

impl Drop for RootClaim {
    fn drop(&mut self) {
        RUNNING.with(|running| running.borrow_mut().remove(&self.0));
        // Called with no borrow held: the start claims this very root.
        let start = WAITING.with(|waiting| waiting.borrow_mut().remove(&self.0));
        if let Some(start) = start {
            start();
        }
    }
}

/// The sentence a second ask for a walking folder gets.
fn already_importing(state: crate::context::LibraryContext, root: &str) {
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

/// Check-and-claim is one write, or the second drops the first.
pub(super) fn claim_root(root: &str, asked: Asked) -> Option<RootClaim> {
    let free = RUNNING.with(|running| {
        running
            .borrow_mut()
            .insert(root.to_string(), asked)
            .is_none()
    });
    free.then(|| RootClaim(root.to_string()))
}

/// Runs `start` now, queues it behind a rescan, or answers `false`.
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

/// Claim, card and refusal in one place for the run doors.
pub(super) fn start_guarded(
    state: crate::context::LibraryContext,
    root: &str,
    launch: impl FnOnce(LibraryContext, String, String) + 'static,
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

/// The claim gate without a card: the same sentence, no card.
pub(super) fn gate_root(
    state: crate::context::LibraryContext,
    root: &str,
    launch: impl FnOnce() + 'static,
) {
    if !when_root_is_free(root, launch) {
        already_importing(state, root);
    }
}
