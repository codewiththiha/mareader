//! How many tasks each saved model runs at once, held as tickets.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mareader_settings::PresetId;
use mareader_study::{Failure, Why};
use tokio::sync::Notify;

/// A task's length, which decides whether it waits for a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Over in seconds, like Explain; it waits a little for others like it.
    Short,
    /// Runs for minutes; names itself, as "organizing your library".
    Long(&'static str),
}

#[derive(Default)]
struct Inner {
    held: Mutex<HashMap<PresetId, Vec<(u64, Kind)>>>,
    next: Mutex<u64>,
    freed: Notify,
}

/// Every model's slots; cheap to clone.
#[derive(Clone, Default)]
pub struct Slots(Arc<Inner>);

/// A slot held; dropping it frees the slot.
pub struct Ticket {
    slots: Arc<Inner>,
    preset: PresetId,
    id: u64,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if let Ok(mut held) = self.slots.held.lock()
            && let Some(list) = held.get_mut(&self.preset)
        {
            list.retain(|(id, _)| *id != self.id);
        }
        self.slots.freed.notify_waiters();
    }
}

impl Slots {
    /// How long a short task waits behind other short ones.
    pub const SHORT_WAIT: Duration = Duration::from_secs(15);

    /// Tasks `preset` runs now.
    pub fn in_use(&self, preset: PresetId) -> usize {
        let held = self.0.held.lock();
        held.map_or(0, |h| h.get(&preset).map_or(0, Vec::len))
    }

    /// The long task holding `preset`, if any.
    pub fn long_holder(&self, preset: PresetId) -> Option<&'static str> {
        let held = self.0.held.lock().ok()?;
        held.get(&preset)?.iter().find_map(|(_, k)| match k {
            Kind::Long(what) => Some(*what),
            Kind::Short => None,
        })
    }

    /// A slot now if one of `cap` is free.
    pub fn try_take(&self, preset: PresetId, cap: usize, kind: Kind) -> Option<Ticket> {
        let mut held = self.0.held.lock().ok()?;
        let list = held.entry(preset).or_default();
        if list.len() >= cap.max(1) {
            return None;
        }
        let id = {
            let mut next = self.0.next.lock().ok()?;
            *next += 1;
            *next
        };
        list.push((id, kind));
        Some(Ticket {
            slots: Arc::clone(&self.0),
            preset,
            id,
        })
    }

    /// A slot, or why not; only a short task behind short ones waits.
    pub async fn take(
        &self,
        preset: PresetId,
        cap: usize,
        kind: Kind,
        name: &str,
        wait: Duration,
    ) -> Result<Ticket, Failure> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let freed = self.0.freed.notified();
            tokio::pin!(freed);
            freed.as_mut().enable();
            if let Some(ticket) = self.try_take(preset, cap, kind) {
                return Ok(ticket);
            }
            let long = self.long_holder(preset);
            if kind != Kind::Short || long.is_some() {
                return Err(busy(name, cap, long));
            }
            if tokio::time::timeout_at(deadline, freed).await.is_err() {
                return Err(busy(name, cap, None));
            }
        }
    }
}

/// Says who holds the model, and how to get more room.
fn busy(name: &str, cap: usize, long: Option<&str>) -> Failure {
    let runs = match cap {
        1 => "runs 1 task at a time".to_owned(),
        n => format!("runs {n} tasks at a time"),
    };
    let doing = long.map_or_else(|| "is busy".to_owned(), |what| format!("is {what}"));
    Failure::detailed(
        Why::InUse,
        format!(
            "“{name}” {doing} and {runs}. Raise “At once” or pick another model in Settings › AI models."
        ),
    )
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::backend::block;

    const P: PresetId = PresetId(1);
    const WAIT: Duration = Duration::from_millis(200);

    #[test]
    fn a_full_model_refuses_and_a_dropped_ticket_frees_it() {
        let slots = Slots::default();
        let first = slots.try_take(P, 1, Kind::Short);
        assert!(first.is_some());
        assert!(slots.try_take(P, 1, Kind::Short).is_none());
        assert!(slots.try_take(PresetId(2), 1, Kind::Short).is_some());
        assert_eq!(slots.in_use(P), 1);
        drop(first);
        assert_eq!(slots.in_use(P), 0);
        let two: Vec<_> = (0..3)
            .filter_map(|_| slots.try_take(P, 2, Kind::Short))
            .collect();
        assert_eq!(two.len(), 2, "a cap of two holds two");
    }

    #[test]
    fn a_long_task_makes_others_fail_at_once_naming_it() {
        let slots = Slots::default();
        let _organizing = slots.try_take(P, 1, Kind::Long("organizing your library"));
        let got = block(slots.take(P, 1, Kind::Short, "Fast", WAIT));
        let f = got.err().expect("refused");
        assert_eq!(f.why, Why::InUse);
        let said = f.detail.unwrap_or_default();
        assert!(said.contains("“Fast” is organizing your library"), "{said}");
        assert!(said.contains("runs 1 task at a time"), "{said}");
        // With room for two, Explain still runs beside it.
        assert!(block(slots.take(P, 2, Kind::Short, "Fast", WAIT)).is_ok());
    }

    #[test]
    fn a_short_task_waits_for_another_short_one() {
        let slots = Slots::default();
        let held = slots.try_take(P, 1, Kind::Short);
        let later = slots.clone();
        let got = block(async move {
            let free = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                drop(held);
            });
            let got = later.take(P, 1, Kind::Short, "Fast", WAIT).await;
            let _ = free.await;
            got
        });
        assert!(got.is_ok(), "it got the slot once freed");
        // Never freed: it gives up after the wait.
        let _stuck = slots.try_take(P, 1, Kind::Short);
        let got = block(slots.take(P, 1, Kind::Short, "Fast", WAIT));
        assert_eq!(got.err().map(|f| f.why), Some(Why::InUse));
    }

    #[test]
    fn a_long_task_never_queues() {
        let slots = Slots::default();
        let _explaining = slots.try_take(P, 1, Kind::Short);
        let got = block(slots.take(P, 1, Kind::Long("organizing"), "Fast", WAIT));
        let said = got.err().and_then(|f| f.detail).unwrap_or_default();
        assert!(said.contains("“Fast” is busy"), "{said}");
    }
}
