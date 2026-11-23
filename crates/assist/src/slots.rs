//! The queue in front of every saved model: who runs, who waits, in what order.
//!
//! A model runs At once tasks on each usable key. Past that, tasks queue:
//! Waited before Asked before Ahead, then first come first served. A ticket
//! is a running task's place; dropping it lets the next one in.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use mareader_settings::{Class, ModelPreset, PresetId, Provider, TaskId};
use tokio::sync::Notify;

/// A task that had to wait behind another, as its toast tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Queued {
    pub task: TaskId,
    /// The model's name.
    pub model: String,
    /// Tasks the model runs at once.
    pub room: usize,
    /// The other task holding the model.
    pub behind: TaskId,
}

impl Queued {
    /// What the reader is told: who holds the model, and what waits.
    pub fn said(&self) -> String {
        let runs = match self.room {
            1 => "1 task".to_owned(),
            n => format!("{n} tasks"),
        };
        let (model, holding) = (&self.model, self.behind.spec().doing);
        let doing = self.task.spec().doing;
        let mut next = doing.chars();
        let first = next.next().map(|c| c.to_uppercase().collect::<String>());
        let next = format!("{}{}", first.unwrap_or_default(), next.as_str());
        let held = format!("“{model}” is {holding} and runs {runs} at a time.");
        format!("{held} {next} starts when it's free.")
    }
}

/// One task in line.
struct Wait {
    id: u64,
    preset: PresetId,
    class: Class,
}

#[derive(Default)]
struct State {
    /// Who runs on each model now.
    held: HashMap<PresetId, Vec<(u64, TaskId)>>,
    /// Who waits, oldest first.
    queued: Vec<Wait>,
    /// Usable keys per provider, once counted.
    lanes: HashMap<Provider, u32>,
    /// Tasks that had to wait, until the app takes them.
    news: Vec<Queued>,
    next: u64,
}

impl State {
    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// Whether a `class` task may start on `preset` beside those running.
    fn fits(&self, preset: PresetId, class: Class, room: usize) -> bool {
        let held = self.held.get(&preset).map_or(0, Vec::len);
        // Work made ahead leaves the last place for someone waiting.
        let limit = match class {
            Class::Ahead if room >= 2 => room - 1,
            _ => room,
        };
        held < limit
    }

    /// Whether `id` is next in line for `preset`.
    fn first(&self, preset: PresetId, id: u64) -> bool {
        let line = self.queued.iter().filter(|w| w.preset == preset);
        let first = line.min_by_key(|w| (w.class, w.id));
        first.is_some_and(|w| w.id == id)
    }

    /// Whether anyone of `class` or before waits for `preset`.
    fn waiting(&self, preset: PresetId, class: Class) -> bool {
        let mut line = self.queued.iter();
        line.any(|w| w.preset == preset && w.class <= class)
    }
}

/// Where a task stands after a look at the line.
enum Turn {
    In(Ticket),
    /// Joined the line as this id, behind this many.
    Joined(u64, usize),
    Waits,
}

#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    freed: Notify,
}

/// Every model's queue; cheap to clone.
#[derive(Clone, Default)]
pub struct Slots(Arc<Inner>);

/// A place held; dropping it frees the place.
pub struct Ticket {
    slots: Arc<Inner>,
    preset: PresetId,
    id: u64,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if let Ok(mut state) = self.slots.state.lock()
            && let Some(list) = state.held.get_mut(&self.preset)
        {
            list.retain(|(id, _)| *id != self.id);
        }
        self.slots.freed.notify_waiters();
    }
}

/// A place in line; a cancelled job leaves the line with it.
struct InLine<'a> {
    of: &'a Inner,
    id: u64,
}

impl Drop for InLine<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.of.state.lock() {
            state.queued.retain(|w| w.id != self.id);
        }
        self.of.freed.notify_waiters();
    }
}

impl Slots {
    /// How long a waited task queues for a model before trying the next.
    pub const FUSE: Duration = Duration::from_secs(15);

    fn state(&self) -> Option<MutexGuard<'_, State>> {
        self.0.state.lock().ok()
    }

    /// Tasks `preset` runs now.
    pub fn in_use(&self, preset: PresetId) -> usize {
        let state = self.state();
        state.map_or(0, |s| s.held.get(&preset).map_or(0, Vec::len))
    }

    /// Tasks waiting for `preset`.
    pub fn queued(&self, preset: PresetId) -> usize {
        let Some(state) = self.state() else {
            return 0;
        };
        state.queued.iter().filter(|w| w.preset == preset).count()
    }

    /// Usable keys of `p`, if counted.
    pub fn lanes(&self, p: Provider) -> Option<u32> {
        self.state().and_then(|s| s.lanes.get(&p).copied())
    }

    /// Takes `p`'s count of usable keys; tasks in line see the new room.
    pub fn count(&self, p: Provider, lanes: u32) {
        if let Some(mut state) = self.state() {
            state.lanes.insert(p, lanes.max(1));
        }
        self.0.freed.notify_waiters();
    }

    /// Tasks `p` may run at once: At once per usable key, once counted.
    pub fn room(&self, p: &ModelPreset) -> usize {
        let lanes = match p.provider.takes_keys() {
            true => self.lanes(p.provider).unwrap_or(1),
            false => 1,
        };
        let room = u32::from(p.at_once).saturating_mul(lanes).max(1);
        usize::try_from(room).unwrap_or(usize::MAX)
    }

    /// Whether a task the reader waits on is in line for `preset`.
    pub fn waited_for(&self, preset: PresetId) -> bool {
        let state = self.state();
        state.is_some_and(|s| s.waiting(preset, Class::Waited))
    }

    /// Tasks that waited behind another since the last call.
    pub fn news(&self) -> Vec<Queued> {
        self.state()
            .map(|mut s| std::mem::take(&mut s.news))
            .unwrap_or_default()
    }

    fn admit(&self, state: &mut State, preset: PresetId, id: u64, task: TaskId) -> Ticket {
        state.held.entry(preset).or_default().push((id, task));
        Ticket {
            slots: Arc::clone(&self.0),
            preset,
            id,
        }
    }

    /// A place now, if `p` has room and no one is in line before `task`.
    pub fn try_take(&self, p: &ModelPreset, task: TaskId) -> Option<Ticket> {
        let (room, class) = (self.room(p), task.spec().class);
        let mut state = self.state()?;
        if state.waiting(p.id, class) || !state.fits(p.id, class, room) {
            return None;
        }
        let id = state.id();
        Some(self.admit(&mut state, p.id, id, task))
    }

    /// A place on `p` at `task`'s turn; none if `fuse` runs out first.
    ///
    /// `queued` hears how many are ahead when the task joins the line.
    pub async fn take(
        &self,
        p: &ModelPreset,
        task: TaskId,
        fuse: Option<Duration>,
        queued: &(dyn Fn(usize) + Sync),
    ) -> Option<Ticket> {
        let deadline = fuse.map(|f| tokio::time::Instant::now() + f);
        let mut line: Option<InLine<'_>> = None;
        loop {
            let freed = self.0.freed.notified();
            tokio::pin!(freed);
            freed.as_mut().enable();
            match self.look(p, task, line.as_ref().map(|l| l.id))? {
                Turn::In(ticket) => return Some(ticket),
                Turn::Joined(id, ahead) => {
                    line = Some(InLine { of: &self.0, id });
                    queued(ahead);
                }
                Turn::Waits => {}
            }
            match deadline {
                Some(at) => tokio::time::timeout_at(at, freed).await.ok()?,
                None => freed.await,
            }
        }
    }

    /// One look at `p`'s line for `task`, in line as `id` if it is.
    fn look(&self, p: &ModelPreset, task: TaskId, id: Option<u64>) -> Option<Turn> {
        let (room, class) = (self.room(p), task.spec().class);
        let mut state = self.state()?;
        let turn = match id {
            Some(id) => state.first(p.id, id),
            None => !state.waiting(p.id, class),
        };
        if turn && state.fits(p.id, class, room) {
            let id = id.unwrap_or_else(|| state.id());
            state.queued.retain(|w| w.id != id);
            return Some(Turn::In(self.admit(&mut state, p.id, id, task)));
        }
        if id.is_some() {
            return Some(Turn::Waits);
        }
        let (id, ahead) = self.join(&mut state, p, task, room);
        Some(Turn::Joined(id, ahead))
    }

    /// Puts `task` in line; its id, and how many run or wait before it.
    fn join(&self, state: &mut State, p: &ModelPreset, task: TaskId, room: usize) -> (u64, usize) {
        let (id, class) = (state.id(), task.spec().class);
        let none = Vec::new();
        let held = state.held.get(&p.id).unwrap_or(&none);
        let other = held.iter().map(|(_, t)| *t).find(|t| *t != task);
        let before = state.queued.iter();
        let before = before.filter(|w| w.preset == p.id && w.class <= class);
        let ahead = held.len() + before.count();
        state.queued.push(Wait {
            id,
            preset: p.id,
            class,
        });
        // A task behind its own kind is no news; another task's is.
        if let Some(behind) = other {
            let model = p.title();
            let news = Queued {
                task,
                model,
                room,
                behind,
            };
            if !state.news.contains(&news) {
                state.news.push(news);
            }
        }
        (id, ahead)
    }
}

#[cfg(test)]
#[path = "slots_tests.rs"]
mod tests;
