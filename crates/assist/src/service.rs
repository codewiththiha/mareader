//! What the app holds: a runtime off the UI thread, clients, jobs.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use fm_bridge::Bridge;
use mareader_settings::{AiSettings, ModelPreset, PresetId, Provider, TaskId, no_model};
use mareader_study::{Explainer, Failure, Why, WordInfo};
use serde_json::Value;
use tokio::runtime::{Builder, Runtime};

use crate::backend::{Backend, BoxFuture, Nobody, Partial};
use crate::board::{AppleState, Board, Job, Reply, Tried, lanes};
use crate::compat::{Compat, failure_with_keys};
use crate::dispatch::{Dispatch, Seat};
use crate::keys::{Clients, pasted, row};
use crate::organize::{Checkpoint, Hooks, Plan, Progress, Request, organize_from};
use crate::page::{PageJob, rewrite};
use crate::prompt::Prompt;
use crate::retry::attempt;
use crate::running::{Event, Running, Say, Waker, start};
use crate::slots::{Queued, Slots};
use crate::task::Explain;
use mareader_study::page::Rewritten;
mod cards;

/// The paused organizer run, and one set aside for Undo.
const CHECKPOINT: &str = "organize.json";
const SET_ASIDE: &str = "organize.old.json";

/// Writes `run` whole or not at all: a crash mid-write keeps the old one.
fn keep(file: &std::path::Path, run: &Checkpoint) {
    let Ok(text) = serde_json::to_string(run) else {
        return;
    };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let draft = file.with_extension("json.part");
    if std::fs::write(&draft, text).is_ok() {
        let _ = std::fs::rename(&draft, file);
    }
}

/// The sample a trial explains.
const SAMPLE: Explain<'static> = Explain {
    word: "quick",
    context: "The quick brown fox jumps over the lazy dog.",
    language: None,
};

/// The runtime, shut down without waiting, from wherever it drops.
struct Engine(Option<Runtime>);

impl Engine {
    fn rt(&self) -> Option<&Runtime> {
        self.0.as_ref()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(rt) = self.0.take() {
            rt.shutdown_background();
        }
    }
}

/// AI for the app: answerers for the study tools, jobs for the settings.
pub struct Assist {
    root: PathBuf,
    engine: Arc<Engine>,
    clients: Arc<Clients>,
    apple: Option<Arc<crate::Apple>>,
    ai: AiSettings,
    waker: Waker,
    board: Board,
    slots: Slots,
    /// Settings jobs under way; their replies land on the board.
    jobs: Vec<Running<Reply>>,
}

impl Assist {
    /// Keys are kept under `root`; `waker` runs as each job ends.
    pub fn new(root: PathBuf, waker: Waker) -> std::io::Result<Self> {
        let rt = Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("mareader-assist")
            .enable_all()
            .build()?;
        Ok(Self {
            engine: Arc::new(Engine(Some(rt))),
            clients: Arc::new(Clients::new(root.clone())),
            root,
            apple: locate_apple(),
            ai: AiSettings::default(),
            waker,
            board: Board::default(),
            slots: Slots::default(),
            jobs: Vec::new(),
        })
    }

    /// Keeps the master key in a file under the root, never the keychain.
    pub fn without_keychain(mut self) -> Self {
        let root = self.root.clone();
        self.clients = Arc::new(Clients::new(root).without_keychain());
        self
    }

    /// Uses the helper at `path` when the app bundle has none.
    pub fn with_helper(mut self, path: PathBuf) -> Self {
        if cfg!(target_os = "macos") && self.apple.is_none() && path.is_file() {
            self.apple = Some(Arc::new(crate::Apple::new(Bridge::new(path))));
        }
        self
    }

    /// Uses `apple` for Apple Intelligence, as tests do with a mock.
    pub fn with_apple(mut self, apple: crate::Apple) -> Self {
        self.apple = Some(Arc::new(apple));
        self
    }

    /// The answerer for `ai`; later jobs use these settings too.
    pub fn configure(&mut self, ai: &AiSettings) -> Arc<dyn Explainer> {
        self.ai = ai.clone();
        let route = self.route(TaskId::Explain);
        Arc::new(AiExplainer {
            line: self.line(),
            by: route.first().map(|s| s.preset.title()).unwrap_or_default(),
            route,
            language: ai.explain_language(),
        })
    }

    /// The models `task` may ask, each with who answers for it.
    fn route(&self, task: TaskId) -> Vec<Seat> {
        let route = self.ai.route(task).into_iter();
        route.map(|p| self.seat(p)).collect()
    }

    fn seat(&self, preset: &ModelPreset) -> Seat {
        Seat {
            preset: preset.clone(),
            backend: self.backend(preset),
        }
    }

    fn line(&self) -> Line {
        Line {
            engine: Arc::clone(&self.engine),
            waker: Arc::clone(&self.waker),
            slots: self.slots.clone(),
        }
    }

    /// Who answers for `preset`, as configured.
    pub fn backend(&self, preset: &ModelPreset) -> Arc<dyn Backend> {
        match (preset.provider, &self.apple) {
            (Provider::Apple, Some(apple)) => Arc::clone(apple) as Arc<dyn Backend>,
            (Provider::Apple, None) => Arc::new(Nobody(no_apple())),
            (server, _) => Arc::new(Server {
                clients: Arc::clone(&self.clients),
                provider: server,
                url: self.ai.url(server),
                model: preset.model().to_owned(),
            }),
        }
    }

    /// Every model's queue.
    pub fn slots(&self) -> &Slots {
        &self.slots
    }

    /// Tasks `preset` runs now.
    pub fn in_use(&self, preset: PresetId) -> usize {
        self.slots.in_use(preset)
    }

    /// Tasks `preset` may run at once: the number the queue lets in.
    pub fn capacity(&self, preset: &ModelPreset) -> u32 {
        u32::try_from(self.slots.room(preset)).unwrap_or(u32::MAX)
    }

    /// Tasks that had to wait behind another since the last call.
    pub fn queued(&self) -> Vec<Queued> {
        self.slots.news()
    }

    /// Plans `req` afresh; once the model is free, it replaces a paused run.
    pub fn organize(&self, req: Request) -> Organizing {
        self.organize_from(req, None)
    }

    /// Carries on the paused run, fitted to `req`; starts one if none.
    pub fn resume(&self, req: Request) -> Organizing {
        let paused = self.paused();
        self.organize_from(req, paused)
    }

    /// The run a failure, a stop or quitting left; kept until forgotten.
    pub fn paused(&self) -> Option<Checkpoint> {
        let text = std::fs::read_to_string(self.root.join(CHECKPOINT)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Sets the paused run aside, so [`Self::bring_back`] can undo it.
    pub fn set_aside(&self) -> bool {
        std::fs::rename(self.root.join(CHECKPOINT), self.root.join(SET_ASIDE)).is_ok()
    }

    /// Brings back the run last set aside.
    pub fn bring_back(&self) -> bool {
        std::fs::rename(self.root.join(SET_ASIDE), self.root.join(CHECKPOINT)).is_ok()
    }

    /// Drops the paused run for good, as when its plan is applied.
    pub fn forget(&self) {
        let _ = std::fs::remove_file(self.root.join(CHECKPOINT));
        let _ = std::fs::remove_file(self.root.join(SET_ASIDE));
    }

    fn organize_from(&self, req: Request, paused: Option<Checkpoint>) -> Organizing {
        let resumed = paused.is_some();
        let run = match paused {
            Some(run) => run.carry_on(&req),
            None => Checkpoint::new(&req),
        };
        let id = TaskId::Organize;
        let seat = self.route(id).into_iter().next();
        if seat.is_none() && !run.finished() {
            let none = Failure::detailed(Why::NotHere, no_model(id));
            return start(self.engine.rt(), &self.waker, |_| async { Err(none) });
        }
        // A finished run only makes its plan; no model is asked.
        let seat = seat.filter(|_| !run.finished());
        let file = self.root.join(CHECKPOINT);
        let job = move |backend: Arc<dyn Backend>, say: Say<Plan, Progress>| async move {
            let save = |run: &Checkpoint| keep(&file, run);
            if !resumed {
                save(&run);
            }
            let hooks = Hooks {
                progress: &|p| say.partial(p),
                save: &save,
                retry: id.spec().retry,
            };
            organize_from(backend.as_ref(), &req, run, &hooks).await
        };
        self.line().dispatch().flow(id, seat, job)
    }

    /// Rewrites one page with its task's model; news comes by polling.
    pub fn rewrite(&self, job: PageJob) -> Rewriting {
        let task = TaskId::from(job.rewrite.task());
        let step = move |backend: Arc<dyn Backend>, retry, say: Say<Rewritten, Rewritten>| {
            let ask = job.clone().ask();
            async move { rewrite(backend.as_ref(), &ask, &|r| say.partial(r), retry).await }
        };
        self.line().dispatch().ask(task, self.route(task), step)
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    /// Takes in finished jobs; true if any changed the board.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let (board, slots) = (&mut self.board, &self.slots);
        self.jobs.retain_mut(|job| {
            let Some(reply) = job.answer() else {
                return true;
            };
            if let Ok(reply) = reply {
                if let Some((provider, lanes)) = reply.lanes() {
                    slots.count(provider, lanes);
                }
                board.finish(reply);
            }
            changed = true;
            false
        });
        changed
    }

    /// Starts `job` in the background; [`Self::poll`] takes its reply.
    pub fn send(&mut self, job: Job) {
        let Some(rt) = self.engine.rt() else {
            return;
        };
        self.board.start(&job);
        let work = Work {
            clients: Arc::clone(&self.clients),
            apple: self.apple.clone(),
            ai: self.ai.clone(),
            trial: match &job {
                Job::Try(p) => Some(self.seat(p)),
                _ => None,
            },
            slots: self.slots.clone(),
        };
        let reply = async move { Ok(work.run(job).await) };
        let job = start(Some(rt), &self.waker, |_| reply);
        self.jobs.push(job);
    }
}

#[cfg(target_os = "macos")]
fn locate_apple() -> Option<Arc<crate::Apple>> {
    crate::Apple::locate().ok().map(Arc::new)
}

#[cfg(not(target_os = "macos"))]
fn locate_apple() -> Option<Arc<crate::Apple>> {
    None
}

fn no_apple() -> Failure {
    let why = match cfg!(target_os = "macos") {
        true => "the FMBridge helper wasn't found; see docs/packaging.md",
        false => "Apple Intelligence runs only on a Mac",
    };
    Failure::detailed(Why::NotHere, why)
}

/// One provider's server, its client built on first use.
struct Server {
    clients: Arc<Clients>,
    provider: Provider,
    url: String,
    model: String,
}

impl Backend for Server {
    fn ask<'a>(
        &'a self,
        prompt: &'a Prompt,
        partial: Partial<'a>,
    ) -> BoxFuture<'a, Result<Value, Failure>> {
        Box::pin(async move {
            let client = self.clients.asking(self.provider, &self.url).await?;
            let compat = Compat::new(client, self.model.as_str());
            compat.ask(prompt, partial).await
        })
    }

    fn lanes(&self) -> BoxFuture<'_, u32> {
        Box::pin(async move {
            if !self.provider.takes_keys() {
                return 1;
            }
            let Ok(client) = self.clients.client(self.provider, &self.url, true).await else {
                return 1;
            };
            match client.manager().list_keys().await {
                Ok(keys) => lanes(&keys.into_iter().map(row).collect::<Vec<_>>()),
                Err(_) => 1,
            }
        })
    }
}

/// News from an organizing run: progress, then the plan.
pub type Organized = Event<Plan, Progress>;

/// A run under way; dropping it cancels the run and frees its model.
pub type Organizing = Running<Plan, Progress>;

/// News from a page rewrite: what came so far, then the result.
pub type Rewrote = Event<Rewritten>;

/// A rewrite under way; dropping it cancels it and frees its model.
pub type Rewriting = Running<Rewritten>;

/// What starts a job: the runtime, the UI's waker and the queue.
struct Line {
    engine: Arc<Engine>,
    waker: Waker,
    slots: Slots,
}

impl Line {
    fn dispatch(&self) -> Dispatch<'_> {
        Dispatch {
            rt: self.engine.rt(),
            waker: &self.waker,
            slots: &self.slots,
        }
    }
}

/// The study tools' answerer: blocks its worker thread, not the UI.
struct AiExplainer {
    line: Line,
    route: Vec<Seat>,
    language: Option<&'static str>,
    by: String,
}

impl Explainer for AiExplainer {
    fn explain(
        &self,
        word: &str,
        context: &str,
        partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure> {
        let language = self.language;
        let (word, context): (Arc<str>, Arc<str>) = (word.into(), context.into());
        let step = move |backend: Arc<dyn Backend>, retry, say: Say<WordInfo, WordInfo>| {
            let (word, context) = (Arc::clone(&word), Arc::clone(&context));
            async move {
                let task = Explain {
                    word: &word,
                    context: &context,
                    language,
                };
                attempt(backend.as_ref(), &task, &|info| say.partial(info), retry).await
            }
        };
        let (id, route) = (TaskId::Explain, self.route.clone());
        let job = self.line.dispatch().ask(id, route, step);
        job.wait_with(partial)
    }

    fn by(&self) -> String {
        self.by.clone()
    }
}

/// What a job needs, moved onto the runtime.
struct Work {
    clients: Arc<Clients>,
    apple: Option<Arc<crate::Apple>>,
    ai: AiSettings,
    trial: Option<Seat>,
    slots: Slots,
}

impl Work {
    async fn run(self, job: Job) -> Reply {
        match job {
            Job::Probe => Reply::Apple(match &self.apple {
                Some(apple) => apple.probe().await,
                None => AppleState::Missing(no_apple().detail.unwrap_or_default()),
            }),
            Job::Keys(p) => self.keys(p, None).await,
            Job::AddKey(p, key) => {
                let problem = match pasted(&key) {
                    Ok(key) => self.change(p, Change::Add(key)).await,
                    Err(f) => Some(f),
                };
                self.keys(p, problem).await
            }
            Job::RemoveKey(p, id) => self.changed(p, Change::Remove(&id)).await,
            Job::BanKey(p, id) => self.changed(p, Change::Ban(&id)).await,
            Job::RecoverKey(p, id) => self.changed(p, Change::Recover(&id)).await,
            Job::Models(p) => Reply::Models(p, self.models(p).await),
            Job::Try(p) => Reply::Tried(p.id, self.trial(&p).await),
        }
    }

    async fn changed(&self, p: Provider, change: Change<'_>) -> Reply {
        let problem = self.change(p, change).await;
        self.keys(p, problem).await
    }

    /// Makes `change`; the failure, if any.
    async fn change(&self, p: Provider, change: Change<'_>) -> Option<Failure> {
        let client = match self.clients.client(p, &self.ai.url(p), true).await {
            Ok(c) => c,
            Err(f) => return Some(f),
        };
        let keys = client.manager();
        let done = match change {
            Change::Add(key) => keys.add_key(key).await.map(drop),
            Change::Remove(id) => keys.remove_key(id).await,
            Change::Ban(id) => keys.ban_key(id).await,
            Change::Recover(id) => keys.recover_key(id).await,
        };
        done.err().map(|e| crate::compat::failure(&e))
    }

    async fn keys(&self, p: Provider, problem: Option<Failure>) -> Reply {
        let keys = match self.clients.client(p, &self.ai.url(p), true).await {
            Ok(client) => client
                .manager()
                .list_keys()
                .await
                .map_err(|e| crate::compat::failure(&e)),
            Err(f) => Err(f),
        };
        Reply::Keys {
            provider: p,
            keys: keys.map(|k| k.into_iter().map(row).collect()),
            problem,
        }
    }

    async fn models(&self, p: Provider) -> Result<Vec<String>, Failure> {
        let client = self.clients.asking(p, &self.ai.url(p)).await?;
        match client.models().await {
            Ok(models) => Ok(models.into_iter().map(|m| m.id).collect()),
            Err(e) => Err(failure_with_keys(&client, e).await),
        }
    }

    async fn trial(&self, preset: &ModelPreset) -> Result<Tried, Failure> {
        let seat = self.trial.as_ref().ok_or(Why::NotHere)?;
        let quiet = |_: usize| {};
        let slot = self.slots.take(preset, TaskId::Explain, None, &quiet).await;
        let _slot = slot.ok_or(Why::InUse)?;
        let start = Instant::now();
        let info = crate::backend::run(seat.backend.as_ref(), &SAMPLE, &|_| {}).await?;
        Ok(Tried {
            millis: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            meaning: info.meaning,
        })
    }
}

enum Change<'a> {
    Add(&'a str),
    Remove(&'a str),
    Ban(&'a str),
    Recover(&'a str),
}
