//! One walk per root: the synchronous claim that keeps two runs off one folder's ledger,
//! and the drop guard that releases it however a run ends.

use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::services::library::{folder_label, toast};
use crate::state::AppState;

use super::Asked;

thread_local! {
    /// One run per root, claimed synchronously and released when the run's future drops — see [`claim_root`].
    static RUNNING: RefCell<HashMap<String, Asked>> = RefCell::new(HashMap::new());
    /// One start per root, run by the rescan's own release rather than polled for, so an ask is never refused by a walk the app started for itself.
    static WAITING: RefCell<HashMap<String, Box<dyn FnOnce()>>> = RefCell::new(HashMap::new());
}

pub(super) struct RootClaim(String);

impl Drop for RootClaim {
    fn drop(&mut self) {
        RUNNING.with(|running| running.borrow_mut().remove(&self.0));
        // Taken out and called with no borrow outstanding on either map: the start it hands over claims this very root.
        let start = WAITING.with(|waiting| waiting.borrow_mut().remove(&self.0));
        if let Some(start) = start {
            start();
        }
    }
}

/// The sentence a second ask for a folder that is already being walked gets. One spelling,
/// because the two doors that can refuse a run refuse it for the same reason.
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
    holder(root).is_some()
        || WAITING.with(|waiting| waiting.borrow().contains_key(root))
}

/// Two concurrent walks of one folder are two snapshots of the same ledger row and two
/// writes back to it, and the second write drops whatever the first run placed. The
/// check-and-claim is one write for that reason.
pub(super) fn claim_root(root: &str, asked: Asked) -> Option<RootClaim> {
    let free = RUNNING.with(|running| {
        running.borrow_mut().insert(root.to_string(), asked).is_none()
    });
    free.then(|| RootClaim(root.to_string()))
}

/// Answers `false` when the root belongs to an ask the READER made — a second import, or
/// a replace — which is the refusal the caller owes a sentence for. The rule: **an ask
/// outranks a rescan.**
pub(super) fn when_root_is_free<F>(root: &str, start: F) -> bool
where
    F: FnOnce() + 'static,
{
    match holder(root) {
        None => {
            start();
            true
        }
        Some(Asked::OnFocus) => WAITING.with(|waiting| {
            match waiting.borrow_mut().entry(root.to_string()) {
                Entry::Occupied(_) => false,
                Entry::Vacant(slot) => {
                    slot.insert(Box::new(start));
                    true
                }
            }
        }),
        Some(Asked::Explicitly) => false,
    }
}
