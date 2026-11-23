//! The one way every AI job runs: routed, queued, retried, reported.
//!
//! A job names its task and the class the reader gave it, which sets its
//! place in line and its retry. Its route is the models it may ask: its own,
//! then the backups. Every job falls through to the next model:
//!
//! | Failure | More models left | Last model |
//! |---|---|---|
//! | Unreadable answer | Re-asked on the same model | Same |
//! | Busy, offline, slow, keys rejected | Next model at once | Class's waits |
//! | 15 s in line, Right away only | Next model | Waits its turn |
//!
//! Other classes wait their turn in line, however long.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use mareader_settings::{AiSettings, Class, ModelPreset, PresetId, Retry, TaskId, no_model};
use mareader_study::{Failure, Why};
use serde_json::Value;
use tokio::runtime::Runtime;

use crate::backend::{Backend, BoxFuture, Nobody, Partial};
use crate::prompt::Prompt;
use crate::running::{Event, Running, Say, Waker, start};
use crate::slots::{Asker, Place, Slots, Ticket};

/// A model in a route, and who answers for it.
#[derive(Clone)]
pub(crate) struct Seat {
    pub preset: ModelPreset,
    pub backend: Arc<dyn Backend>,
}

/// A job queued or running, as Settings lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobInfo {
    pub task: TaskId,
    pub preset: PresetId,
    /// The model's name in the settings followed; blank once removed.
    pub model: String,
    pub running: bool,
    /// When it started, or joined the line.
    pub since: Instant,
}

/// The runtime, shut down without waiting, from wherever it drops.
struct Engine(Option<Runtime>);

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(rt) = self.0.take() {
            rt.shutdown_background();
        }
    }
}

/// The settings last followed, and every task's route under them.
#[derive(Default)]
pub(crate) struct Followed {
    pub ai: AiSettings,
    routes: Vec<(TaskId, Vec<Seat>)>,
}

impl Followed {
    /// The models `id` may ask, first choice first.
    pub fn route(&self, id: TaskId) -> Vec<Seat> {
        let mut routes = self.routes.iter();
        let route = routes.find(|(task, _)| *task == id);
        route.map(|(_, seats)| seats.clone()).unwrap_or_default()
    }

    /// `id` at the class the reader gave it.
    pub fn asker(&self, id: TaskId) -> Asker {
        let class = self.ai.class(id);
        Asker { task: id, class }
    }

    /// The name of `id`'s first model; blank for none.
    pub fn by(&self, id: TaskId) -> String {
        let route = self.routes.iter().find(|(task, _)| *task == id);
        let first = route.and_then(|(_, seats)| seats.first());
        first.map(|s| s.preset.title()).unwrap_or_default()
    }
}

/// What every job starts from; cheap to clone. A port reads the settings
/// when its job starts, so one swap reaches them all.
#[derive(Clone)]
pub(crate) struct Dispatcher {
    engine: Arc<Engine>,
    pub waker: Waker,
    pub slots: Slots,
    now: Arc<Mutex<Arc<Followed>>>,
}

impl Dispatcher {
    pub fn new(rt: Runtime, waker: Waker) -> Self {
        Self {
            engine: Arc::new(Engine(Some(rt))),
            waker,
            slots: Slots::default(),
            now: Arc::default(),
        }
    }

    pub fn rt(&self) -> Option<&Runtime> {
        self.engine.0.as_ref()
    }

    /// New settings; jobs not yet started use them. `seat` makes a backend.
    pub fn follow(&self, ai: &AiSettings, seat: impl Fn(&ModelPreset) -> Seat) {
        let route = |id: TaskId| (id, ai.route(id).into_iter().map(&seat).collect::<Vec<_>>());
        let followed = Followed {
            ai: ai.clone(),
            routes: TaskId::ALL.iter().map(|id| route(*id)).collect(),
        };
        if let Ok(mut now) = self.now.lock() {
            *now = Arc::new(followed);
        }
    }

    /// The settings followed now.
    pub fn followed(&self) -> Arc<Followed> {
        let now = self.now.lock().ok();
        now.map(|now| Arc::clone(&now)).unwrap_or_default()
    }

    /// Everything queued or running: those running first, then the line.
    pub fn jobs(&self) -> Vec<JobInfo> {
        let now = self.followed();
        let name = |id| now.ai.preset(id).map(ModelPreset::title);
        let info = |p: Place| JobInfo {
            task: p.task,
            preset: p.preset,
            model: name(p.preset).unwrap_or_default(),
            running: p.running,
            since: p.since,
        };
        self.slots.places().into_iter().map(info).collect()
    }

    pub fn dispatch(&self) -> Dispatch<'_> {
        Dispatch {
            rt: self.rt(),
            waker: &self.waker,
            slots: &self.slots,
        }
    }
}

/// Starts jobs on the runtime, through the queue.
pub(crate) struct Dispatch<'a> {
    pub rt: Option<&'a Runtime>,
    pub waker: &'a Waker,
    pub slots: &'a Slots,
}

impl Dispatch<'_> {
    /// One request: `step` asks one model, and is called again per model.
    pub fn ask<O, P, S, Fut>(
        &self,
        who: impl Into<Asker>,
        route: Vec<Seat>,
        step: S,
    ) -> Running<O, P>
    where
        O: Send + 'static,
        P: Send + 'static,
        S: Fn(Arc<dyn Backend>, Retry, Say<O, P>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<O, Failure>> + Send + 'static,
    {
        let (slots, who) = (self.slots.clone(), who.into());
        let job = move |say| asked(slots, who, route, step, say);
        start(self.rt, self.waker, job)
    }

    /// Many requests under one place on a model, as the organizer makes.
    ///
    /// The flow asks through a lane that steps aside between requests when
    /// a job the reader waits on is in line, and moves down `route` when a
    /// model fails. No route: no model is asked.
    pub fn flow<O, P, F, Fut>(
        &self,
        who: impl Into<Asker>,
        route: Vec<Seat>,
        flow: F,
    ) -> Running<O, P>
    where
        O: Send + 'static,
        P: Send + 'static,
        F: FnOnce(Arc<dyn Backend>, Say<O, P>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<O, Failure>> + Send + 'static,
    {
        let (slots, who) = (self.slots.clone(), who.into());
        start(self.rt, self.waker, move |say| async move {
            let Some(seat) = route.first() else {
                let none: Arc<dyn Backend> = Arc::new(Nobody(unrouted(who.task)));
                return flow(none, say).await;
            };
            counted(&slots, seat).await;
            let told = |n| say.send(queued(n));
            let ticket = slots.take(&seat.preset, who, None, &told).await;
            let ticket = ticket.ok_or(Why::InUse)?;
            say.send(Event::Started {
                by: seat.preset.title(),
            });
            let lane = Lane {
                route,
                slots,
                who,
                on: Mutex::new((0, Some(ticket))),
            };
            flow(Arc::new(lane), say).await
        })
    }
}

fn queued<O, P>(behind: usize) -> Event<O, P> {
    Event::Queued { behind }
}

/// What a task with no model fails with. Explain's card already says it.
fn unrouted(id: TaskId) -> Failure {
    match id {
        TaskId::Explain => Why::NotHere.into(),
        _ => Failure::detailed(Why::NotHere, no_model(id)),
    }
}

/// Counts the keys of `seat`'s provider the first time it is asked.
async fn counted(slots: &Slots, seat: &Seat) {
    let provider = seat.preset.provider;
    if provider.takes_keys() && slots.lanes(provider).is_none() {
        slots.count(provider, seat.backend.lanes().await);
    }
}

async fn asked<O, P, S, Fut>(
    slots: Slots,
    who: Asker,
    route: Vec<Seat>,
    step: S,
    say: Say<O, P>,
) -> Result<O, Failure>
where
    O: Send + 'static,
    P: Send + 'static,
    S: Fn(Arc<dyn Backend>, Retry, Say<O, P>) -> Fut,
    Fut: Future<Output = Result<O, Failure>>,
{
    let retry = who.class.retry();
    let (count, mut tried) = (route.len(), Vec::new());
    let mut last = unrouted(who.task);
    for (i, seat) in route.into_iter().enumerate() {
        let more = i + 1 < count;
        let name = seat.preset.title();
        counted(&slots, &seat).await;
        // Only a job the reader waits on gives up on a model that's busy.
        let fuse = (more && who.class == Class::Waited).then_some(Slots::FUSE);
        let told = |n| say.send(queued(n));
        let Some(_ticket) = slots.take(&seat.preset, who, fuse, &told).await else {
            last = Failure::detailed(Why::InUse, format!("“{name}” stayed busy"));
            tried.push(name);
            continue;
        };
        say.send(Event::Started { by: name.clone() });
        let retry = match more {
            true => Retry {
                waits: &[],
                ..retry
            },
            false => retry,
        };
        let again = say.clone();
        let backend = Counting {
            inner: seat.backend,
            again: Box::new(move |why, attempt| again.send(Event::Again { why, attempt })),
            last: Mutex::new((0, None)),
        };
        tried.push(name);
        match step(Arc::new(backend), retry, say.clone()).await {
            Ok(out) => return Ok(out),
            Err(f) if more && moves_on(&f) => last = f,
            Err(f) => return Err(named(f, &tried)),
        }
    }
    Err(named(last, &tried))
}

/// Failures the next model may not share.
fn moves_on(f: &Failure) -> bool {
    use Why::{Busy, KeysRejected, Offline, TimedOut};
    matches!(f.why, Busy | Offline | TimedOut | KeysRejected)
}

/// `f`, naming every model asked when there was more than one.
fn named(mut f: Failure, tried: &[String]) -> Failure {
    if tried.len() < 2 {
        return f;
    }
    let names: Vec<String> = tried.iter().map(|n| format!("“{n}”")).collect();
    let asked = format!("asked {}", names.join(", then "));
    f.detail = Some(match f.detail.take() {
        Some(detail) => format!("{detail}; {asked}"),
        None => asked,
    });
    f
}

/// A seat's backend that tells the job each time it is asked again.
struct Counting {
    inner: Arc<dyn Backend>,
    again: Box<dyn Fn(Why, u8) + Send + Sync>,
    /// Asks so far, and why the last one failed.
    last: Mutex<(u8, Option<Why>)>,
}

impl Backend for Counting {
    fn ask<'a>(
        &'a self,
        prompt: &'a Prompt,
        partial: Partial<'a>,
    ) -> BoxFuture<'a, Result<Value, Failure>> {
        let seen = self.last.lock().ok().map(|mut last| {
            last.0 = last.0.saturating_add(1);
            *last
        });
        // An answer that came but couldn't be read is why it's asked again.
        if let Some((attempt @ 2.., why)) = seen {
            (self.again)(why.unwrap_or(Why::BadAnswer), attempt);
        }
        Box::pin(async move {
            let got = self.inner.ask(prompt, partial).await;
            if let Ok(mut last) = self.last.lock() {
                last.1 = got.as_ref().err().map(|f| f.why);
            }
            got
        })
    }

    fn lanes(&self) -> BoxFuture<'_, u32> {
        self.inner.lanes()
    }
}

/// A flow's line to its models: it holds a place on one, gives it back
/// between requests while a job the reader waits on is in line, and moves
/// to the next model when one fails.
struct Lane {
    route: Vec<Seat>,
    slots: Slots,
    who: Asker,
    /// The model asked now, and its place.
    on: Mutex<(usize, Option<Ticket>)>,
}

impl Lane {
    fn at(&self) -> usize {
        self.on.lock().map_or(0, |on| on.0)
    }

    /// Gives up the place held and takes one on the `at`th model.
    async fn sit(&self, at: usize) {
        let held = self.on.lock().ok().and_then(|mut on| on.1.take());
        drop(held);
        let Some(seat) = self.route.get(at) else {
            return;
        };
        counted(&self.slots, seat).await;
        let quiet = |_: usize| {};
        let back = self.slots.take(&seat.preset, self.who, None, &quiet).await;
        if let Ok(mut on) = self.on.lock() {
            *on = (at, back);
        }
    }
}

impl Backend for Lane {
    fn ask<'a>(
        &'a self,
        prompt: &'a Prompt,
        partial: Partial<'a>,
    ) -> BoxFuture<'a, Result<Value, Failure>> {
        Box::pin(async move {
            let mut at = self.at();
            let preset = self.route.get(at).map(|s| s.preset.id);
            if preset.is_some_and(|id| self.slots.waited_for(id)) {
                self.sit(at).await;
            }
            loop {
                let seat = self.route.get(at).ok_or(Why::NotHere)?;
                match seat.backend.ask(prompt, partial).await {
                    Err(f) if at + 1 < self.route.len() && moves_on(&f) => {
                        at += 1;
                        self.sit(at).await;
                    }
                    got => return got,
                }
            }
        })
    }

    fn lanes(&self) -> BoxFuture<'_, u32> {
        match self.route.get(self.at()) {
            Some(seat) => seat.backend.lanes(),
            None => Box::pin(async { 1 }),
        }
    }
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
