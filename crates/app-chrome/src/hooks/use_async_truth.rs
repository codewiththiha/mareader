//! Async command, synchronous truth: the second half of "the hide always
//! lands".
//!
//! A hover machine decides synchronously, but some surfaces are switched by
//! an async command — the native macOS traffic lights go through Tauri IPC.
//! Between the decision and the command landing the truth can move, and the
//! stale command would then settle the native side last, with nothing left
//! to re-run the effect: lights up over a window that has no bar showing.
//!
//! The fix is the same shape every time, so it lives here: record what was
//! sent, await the command, re-read the live truth, and answer a mismatch
//! with one more command. Read the truth UNTRACKED inside the probe — it
//! runs after the owner's effect has finished, where a tracked read would
//! subscribe an owner that will not run again.

use std::future::Future;
use std::rc::Rc;

use leptos::prelude::*;

/// A verified async switch. `P` is whatever payload the command needs
/// alongside the boolean (the lights carry their header height).
#[derive(Clone)]
pub struct AsyncTruth<P>
where
    P: Clone + 'static,
{
    last_sent: StoredValue<Option<bool>, LocalStorage>,
    send: Rc<dyn Fn(bool, P)>,
}

impl<P> AsyncTruth<P>
where
    P: Clone + 'static,
{
    /// The last decision handed to the command — including the correction a
    /// verification pass sent. Callers gate on it so an unchanged state
    /// costs no IPC.
    pub fn last_sent(&self) -> Option<bool> {
        self.last_sent.try_get_value().flatten()
    }

    /// Send one command and verify it landed.
    pub fn send(&self, want: bool, payload: P) {
        (self.send)(want, payload);
    }
}

/// Build a verified switch owned by the current reactive owner: `command`
/// performs the side effect, `truth` reports what the state should be right
/// now (untracked reads only).
pub fn use_async_truth<P, Fut>(
    truth: impl Fn() -> bool + 'static,
    command: impl Fn(bool, P) -> Fut + 'static,
) -> AsyncTruth<P>
where
    P: Clone + 'static,
    Fut: Future<Output = ()> + 'static,
{
    let last_sent = StoredValue::new_local(None::<bool>);
    let truth = Rc::new(truth);
    let command = Rc::new(command);

    let send: Rc<dyn Fn(bool, P)> = Rc::new(move |want, payload: P| {
        last_sent.try_set_value(Some(want));
        let truth = Rc::clone(&truth);
        let command = Rc::clone(&command);
        wasm_bindgen_futures::spawn_local(async move {
            command(want, payload.clone()).await;
            // The decision may have moved while the command was in flight.
            let now = truth();
            if now != want {
                last_sent.try_set_value(Some(now));
                command(now, payload).await;
            }
        });
    });

    AsyncTruth { last_sent, send }
}
