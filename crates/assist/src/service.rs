//! What the app holds: a runtime off the UI thread, clients, jobs.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use fm_bridge::Bridge;
use mareader_settings::{AiSettings, ModelPreset, PresetId, Provider};
use mareader_study::{Explainer, Failure, Why, WordInfo};
use serde_json::Value;
use tokio::runtime::{Builder, Runtime};

use crate::backend::{Backend, BoxFuture, Nobody, Partial, run};
use crate::board::{AppleState, Board, Job, Reply, Tried, lanes};
use crate::compat::{Compat, failure_with_keys};
use crate::keys::{Clients, pasted, row};
use crate::organize::{Checkpoint, Hooks, Plan, Progress, Request, WAITS, organize_from};
use crate::page::{PageJob, rewrite};
use crate::prompt::Prompt;
use crate::running::Waker;
use crate::running::{Event, Running, start};
use crate::slots::{Kind, Slots, Ticket};
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
        let preset = ai.explain_preset().cloned();
        let backend = match &preset {
            Some(p) => self.backend(p),
            None => Arc::new(Nobody(Why::NotHere.into())),
        };
        Arc::new(AiExplainer {
            engine: Arc::clone(&self.engine),
            waker: Arc::clone(&self.waker),
            slots: self.slots.clone(),
            by: preset.as_ref().map(ModelPreset::title).unwrap_or_default(),
            preset,
            backend,
            language: ai.explain_language(),
        })
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

    /// Every model's slots, shared with long tasks.
    pub fn slots(&self) -> &Slots {
        &self.slots
    }

    /// Tasks `preset` runs now.
    pub fn in_use(&self, preset: PresetId) -> usize {
        self.slots.in_use(preset)
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
        let preset = self.ai.organize_preset().cloned();
        let backend = preset.as_ref().map(|p| self.backend(p));
        let slots = self.slots.clone();
        let file = self.root.join(CHECKPOINT);
        start(self.engine.rt(), &self.waker, |say| async move {
            let save = |run: &Checkpoint| keep(&file, run);
            // A finished run only makes its plan; no model is asked.
            let (backend, slot) = match (preset, backend) {
                _ if run.finished() => {
                    let none: Arc<dyn Backend> = Arc::new(Nobody(Why::NotHere.into()));
                    (none, None)
                }
                (Some(preset), Some(backend)) => {
                    let what = Kind::Long(ORGANIZING);
                    let ticket = slot(&slots, &preset, backend.as_ref(), what).await?;
                    (backend, Some(ticket))
                }
                _ => {
                    let none = "no model is chosen to organize with; pick one in Settings › AI";
                    return Err(Failure::detailed(Why::NotHere, none));
                }
            };
            if !resumed {
                save(&run);
            }
            let hooks = Hooks {
                progress: &|p| say.partial(p),
                save: &save,
                waits: &WAITS,
            };
            let plan = organize_from(backend.as_ref(), &req, run, &hooks).await;
            drop(slot);
            plan
        })
    }

    /// Rewrites one page with its task's model; news comes by polling.
    pub fn rewrite(&self, job: PageJob) -> Rewriting {
        let preset = self.ai.page_preset(job.rewrite.task()).cloned();
        let backend = preset.as_ref().map(|p| self.backend(p));
        let slots = self.slots.clone();
        start(self.engine.rt(), &self.waker, |say| async move {
            let (Some(preset), Some(backend)) = (preset, backend) else {
                let none = "no model is chosen for this; pick one in Settings › AI";
                return Err(Failure::detailed(Why::NotHere, none));
            };
            let ticket = short_slot(&slots, &preset, backend.as_ref()).await?;
            let ask = job.ask();
            let done = rewrite(backend.as_ref(), &ask, &|r| say.partial(r), &WAITS).await;
            drop(ticket);
            done
        })
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    /// Takes in finished jobs; true if any changed the board.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let board = &mut self.board;
        self.jobs.retain_mut(|job| {
            let Some(reply) = job.answer() else {
                return true;
            };
            if let Ok(reply) = reply {
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
                Job::Try(p) => Some(self.backend(p)),
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

/// How a long task names itself to the tasks it turns away.
const ORGANIZING: &str = "organizing your library";

/// Holds a slot of `preset`; how many is `at_once` per lane.
async fn slot(
    slots: &Slots,
    preset: &ModelPreset,
    backend: &dyn Backend,
    kind: Kind,
) -> Result<Ticket, Failure> {
    let lanes = backend.lanes().await;
    let cap = u32::from(preset.at_once).saturating_mul(lanes);
    let cap = usize::try_from(cap).unwrap_or(usize::MAX);
    let wait = Slots::SHORT_WAIT;
    slots
        .take(preset.id, cap, kind, &preset.title(), wait)
        .await
}

async fn short_slot(
    slots: &Slots,
    preset: &ModelPreset,
    backend: &dyn Backend,
) -> Result<Ticket, Failure> {
    slot(slots, preset, backend, Kind::Short).await
}

/// News from an organizing run: progress, then the plan.
pub type Organized = Event<Plan, Progress>;

/// A run under way; dropping it cancels the run and frees its model.
pub type Organizing = Running<Plan, Progress>;

/// News from a page rewrite: what came so far, then the result.
pub type Rewrote = Event<Rewritten>;

/// A rewrite under way; dropping it cancels it and frees its model.
pub type Rewriting = Running<Rewritten>;

/// The study tools' answerer: blocks its worker thread, not the UI.
struct AiExplainer {
    engine: Arc<Engine>,
    waker: Waker,
    slots: Slots,
    preset: Option<ModelPreset>,
    backend: Arc<dyn Backend>,
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
        let (preset, slots) = (self.preset.clone(), self.slots.clone());
        let (backend, language) = (Arc::clone(&self.backend), self.language);
        let (word, context) = (word.to_owned(), context.to_owned());
        let job = start(self.engine.rt(), &self.waker, |say| async move {
            let _slot = match &preset {
                Some(p) => Some(short_slot(&slots, p, backend.as_ref()).await?),
                None => None,
            };
            let task = Explain {
                word: &word,
                context: &context,
                language,
            };
            run(backend.as_ref(), &task, &|info| say.partial(info)).await
        });
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
    trial: Option<Arc<dyn Backend>>,
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
        let backend = self.trial.as_ref().ok_or(Why::NotHere)?;
        let _slot = short_slot(&self.slots, preset, backend.as_ref()).await?;
        let start = Instant::now();
        let info = run(backend.as_ref(), &SAMPLE, &|_| {}).await?;
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
