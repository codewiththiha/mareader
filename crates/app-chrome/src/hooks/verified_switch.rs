//! Async command, synchronous truth: send, verify, correct once.

use std::future::Future;
use std::rc::Rc;

use leptos::prelude::*;

/// A verified async switch, with whatever payload the command needs.
#[derive(Clone)]
pub struct VerifiedSwitch<P>
where
    P: Clone + 'static,
{
    last_sent: StoredValue<Option<bool>, LocalStorage>,
    send: Rc<dyn Fn(bool, P)>,
}

impl<P> VerifiedSwitch<P>
where
    P: Clone + 'static,
{
    /// The last decision handed to the command.
    pub fn last_sent(&self) -> Option<bool> {
        self.last_sent.try_get_value().flatten()
    }

    /// Forget what was sent, so the next decision goes out regardless.
    pub fn forget(&self) {
        self.last_sent.try_set_value(None);
    }

    /// Send one command and verify it landed.
    pub fn send(&self, want: bool, payload: P) {
        (self.send)(want, payload);
    }
}

/// Build a verified switch owned by the current reactive owner.
pub fn use_verified_switch<P, Fut>(
    truth: impl Fn() -> Option<bool> + 'static,
    command: impl Fn(bool, P) -> Fut + 'static,
) -> VerifiedSwitch<P>
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
            let Some(now) = truth() else {
                return;
            };
            if now != want {
                last_sent.try_set_value(Some(now));
                command(now, payload).await;
            }
        });
    });

    VerifiedSwitch { last_sent, send }
}
