//! The registry: one transport task per id, and the calls that steer it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use reqwest::Client;
use tokio::sync::oneshot;

use crate::Phase;
use crate::flags::Flags;
use crate::host::Host;
use crate::job::Job;
use crate::partial::discard;
use crate::progress::{Outcome, Progress};
use crate::transfer::{self, Step};

/// Attempts against one mirror before the next mirror is spent.
const ATTEMPTS_PER_SOURCE: u32 = 2;

/// The first backoff; it doubles per attempt and stops growing at the ceiling.
const BACKOFF_BASE: Duration = Duration::from_millis(500);
const BACKOFF_CEIL: Duration = Duration::from_secs(8);

/// A backoff is slept in slices, so a pause or a cancel lands during it.
const BACKOFF_SLICE: Duration = Duration::from_millis(150);

/// A mirror that cannot be reached is spent quickly, not slowly.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// A read that produces nothing for this long is a stalled mirror.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Records held at once; the oldest finished ones are evicted first.
const SLOT_CAP: usize = 32;

/// One download's record: its spec, its snapshot, and who is waiting.
struct Slot {
    job: Job,
    dest: PathBuf,
    flags: Arc<Flags>,
    progress: Progress,
    /// Insertion order, which the cap evicts by.
    seq: u64,
    /// Set while a transport task owns this id: one task, never two.
    running: bool,
    outcome: Option<Result<Outcome, String>>,
    waiters: Vec<oneshot::Sender<Result<Outcome, String>>>,
}

/// How the mirror loop ended.
enum Verdict {
    Landed(Outcome),
    Stopped(Phase),
    Failed(String),
}

/// The handle a `start` or a `resume` gives back.
pub struct Receipt {
    id: String,
    finished: Option<oneshot::Receiver<Result<Outcome, String>>>,
}

impl Receipt {
    /// The id every later call and event uses.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The adopted file once it ends; a pause is not an ending.
    pub async fn finished(mut self) -> Result<Outcome, String> {
        let Some(receiver) = self.finished.take() else {
            return Err("this receipt was already awaited".into());
        };
        receiver
            .await
            .unwrap_or_else(|_| Err(format!("download '{}' was removed", self.id)))
    }
}

/// Every download one process owns, keyed by id and shared by every feature.
#[derive(Clone, Default)]
pub struct Downloads {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
    next_seq: Arc<AtomicU64>,
}

impl Downloads {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin `job`; a record whose transport still runs is refused.
    pub fn start<H: Host>(&self, host: &H, job: Job) -> Result<Receipt, String> {
        job.check()?;
        let dest = job
            .dest()
            .ok_or_else(|| format!("download '{}' names no file", job.id()))?;
        let id = job.id().to_string();
        let (sender, receiver) = oneshot::channel();
        {
            let mut slots = self.lock()?;
            match slots.get_mut(&id) {
                Some(slot) if slot.running => {
                    return Err(format!("download '{id}' is already running"));
                }
                Some(slot) => {
                    Self::wake(std::mem::take(&mut slot.waiters), &id, "superseded");
                }
                None => Self::evict(&mut slots),
            }
            slots.insert(
                id.clone(),
                Slot {
                    job,
                    dest,
                    flags: Arc::new(Flags::default()),
                    progress: Progress::new(&id),
                    seq: self.next_seq.fetch_add(1, Ordering::Relaxed),
                    running: true,
                    outcome: None,
                    waiters: vec![sender],
                },
            );
        }
        self.publish(host, &id);
        self.spawn(host, id.clone());
        Ok(Receipt {
            id,
            finished: Some(receiver),
        })
    }

    /// Continue a paused download from the bytes already on disk.
    pub fn resume<H: Host>(&self, host: &H, id: &str) -> Result<Receipt, String> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut slots = self.lock()?;
            let Some(slot) = slots.get_mut(id) else {
                return Err(format!("no download '{id}'"));
            };
            if slot.running {
                return Err(format!("download '{id}' is already running"));
            }
            if slot.progress.phase != Phase::Paused {
                return Err(format!("download '{id}' is not paused"));
            }
            slot.flags.release();
            slot.running = true;
            slot.progress.phase = Phase::Preparing;
            slot.progress.message = None;
            slot.waiters.push(sender);
        }
        self.publish(host, id);
        self.spawn(host, id.to_string());
        Ok(Receipt {
            id: id.to_string(),
            finished: Some(receiver),
        })
    }

    /// Stop reading; the partial stays and `resume` continues from it.
    pub fn pause(&self, id: &str) {
        self.flag(id, Flags::pause);
    }

    /// Stop for good; the partial stays, so a later `start` resumes from it.
    pub fn cancel(&self, id: &str) {
        self.flag(id, Flags::cancel);
    }

    /// Drop the record, the partial and the adopted file.
    pub fn remove(&self, id: &str) {
        let Some(slot) = self.lock().ok().and_then(|mut slots| slots.remove(id)) else {
            return;
        };
        slot.flags.cancel();
        Self::wake(slot.waiters, id, "removed");
        discard(&slot.dest);
        let _ = std::fs::remove_file(&slot.dest);
    }

    /// The snapshot of one download.
    pub fn status(&self, id: &str) -> Option<Progress> {
        self.lock().ok()?.get(id).map(|slot| slot.progress.clone())
    }

    /// How one download ended, kept after it ended.
    pub fn outcome(&self, id: &str) -> Option<Result<Outcome, String>> {
        self.lock().ok()?.get(id)?.outcome.clone()
    }

    /// Every snapshot, for a frontend that is booting.
    pub fn list(&self) -> Vec<Progress> {
        self.lock()
            .map(|slots| slots.values().map(|slot| slot.progress.clone()).collect())
            .unwrap_or_default()
    }

    fn flag(&self, id: &str, set: fn(&Flags)) {
        if let Ok(slots) = self.slots.lock()
            && let Some(slot) = slots.get(id)
        {
            set(&slot.flags);
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, HashMap<String, Slot>>, String> {
        self.slots
            .lock()
            .map_err(|_| "download registry poisoned".to_string())
    }

    fn spawn<H: Host>(&self, host: &H, id: String) {
        let registry = self.clone();
        let runner = host.clone();
        let task_host = host.clone();
        runner.spawn(async move { registry.run(&task_host, id).await });
    }

    fn contains(&self, id: &str) -> bool {
        self.lock()
            .map(|slots| slots.contains_key(id))
            .unwrap_or(false)
    }

    fn parts(&self, id: &str) -> Option<(Job, PathBuf, Arc<Flags>)> {
        let slots = self.lock().ok()?;
        let slot = slots.get(id)?;
        Some((slot.job.clone(), slot.dest.clone(), slot.flags.clone()))
    }

    /// Tell everyone waiting on `id` that no ending is coming.
    fn wake(waiters: Vec<oneshot::Sender<Result<Outcome, String>>>, id: &str, why: &str) {
        for waiter in waiters {
            let _ = waiter.send(Err(format!("download '{id}' was {why}")));
        }
    }

    /// Drop the oldest finished records until one more fits.
    fn evict(slots: &mut HashMap<String, Slot>) {
        if slots.len() < SLOT_CAP {
            return;
        }
        let mut finished: Vec<(u64, String)> = slots
            .iter()
            .filter(|(_, slot)| slot.progress.phase.terminal())
            .map(|(id, slot)| (slot.seq, id.clone()))
            .collect();
        finished.sort_unstable();
        let over = slots.len() - SLOT_CAP + 1;
        for (_, id) in finished.into_iter().take(over) {
            slots.remove(&id);
        }
    }
}

impl Downloads {
    /// One transport: the cache answer, else every mirror and every attempt.
    async fn run<H: Host>(&self, host: &H, id: String) {
        let Some((job, dest, flags)) = self.parts(&id) else {
            return;
        };

        // A complete file on disk is the answer; ask the network nothing.
        if !job.force && dest.is_file() {
            match transfer::verify(&job.verify, &dest).await {
                Ok(()) => {
                    let bytes = std::fs::metadata(&dest).map(|meta| meta.len()).unwrap_or(0);
                    self.finish(
                        host,
                        &id,
                        Phase::Done,
                        Ok(Outcome {
                            id: id.clone(),
                            path: dest,
                            bytes,
                            source: 0,
                            cached: true,
                        }),
                    );
                    return;
                }
                // A file that fails its own check is not this download's.
                Err(_) => {
                    let _ = tokio::fs::remove_file(&dest).await;
                }
            }
        }

        let client = match Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                self.failed(host, &id, format!("client: {error}"));
                return;
            }
        };
        if let Err(error) = tokio::fs::create_dir_all(job.dir()).await {
            self.failed(
                host,
                &id,
                format!("create {}: {error}", job.dir().display()),
            );
            return;
        }

        match self
            .transport(host, &client, &id, &job, &dest, &flags)
            .await
        {
            Verdict::Landed(outcome) => self.finish(host, &id, Phase::Done, Ok(outcome)),
            Verdict::Failed(message) => self.failed(host, &id, message),
            Verdict::Stopped(phase) => {
                if !self.contains(&id) {
                    // Removed mid-flight: this task clears its own files.
                    discard(&dest);
                } else if phase.terminal() {
                    self.finish(
                        host,
                        &id,
                        Phase::Cancelled,
                        Err("the download was cancelled".into()),
                    );
                } else {
                    self.settle(host, &id, phase);
                }
            }
        }
    }

    /// The mirror loop: every source, every attempt, until one of them lands.
    async fn transport<H: Host>(
        &self,
        host: &H,
        client: &Client,
        id: &str,
        job: &Job,
        dest: &Path,
        flags: &Flags,
    ) -> Verdict {
        let mut last = String::from("no mirror answered");
        'mirror: for (index, url) in job.sources().iter().enumerate() {
            let source = index as u32;
            for attempt in 1..=ATTEMPTS_PER_SOURCE {
                if !self.contains(id) {
                    return stopped(flags);
                }
                if let Some(phase) = flags.stop() {
                    return Verdict::Stopped(phase);
                }
                self.update(host, id, |progress| {
                    progress.phase = Phase::Downloading;
                    progress.source = Some(source);
                    progress.attempt = attempt;
                    progress.message = None;
                });
                let note = |received: u64, total: Option<u64>, speed: Option<f64>| {
                    self.update(host, id, |progress| {
                        progress.received = received;
                        progress.total = total;
                        if let Some(speed) = speed {
                            progress.speed = Some(speed);
                            progress.eta_secs = eta(received, total, Some(speed));
                        }
                    });
                };
                let step = transfer::attempt(client, url, source, dest, flags, &note).await;
                match step {
                    Step::Complete => {
                        if !self.contains(id) {
                            // Removed as the last bytes landed: the
                            // adoption must not bring it back.
                            discard(dest);
                            return Verdict::Stopped(Phase::Cancelled);
                        }
                        self.update(host, id, |progress| {
                            progress.phase = Phase::Verifying;
                            progress.message = None;
                        });
                        return match transfer::adopt(dest, &job.verify).await {
                            Ok(bytes) => Verdict::Landed(Outcome {
                                id: id.to_string(),
                                path: dest.to_path_buf(),
                                bytes,
                                source,
                                cached: false,
                            }),
                            Err(message) => Verdict::Failed(message),
                        };
                    }
                    Step::Stopped(phase) => return Verdict::Stopped(phase),
                    Step::Transient(message) => {
                        last = message.clone();
                        self.retry(host, id, message);
                        if attempt == ATTEMPTS_PER_SOURCE {
                            continue 'mirror;
                        }
                        backoff(flags, attempt).await;
                    }
                    Step::Permanent(message) => {
                        last = message.clone();
                        self.retry(host, id, message);
                        continue 'mirror;
                    }
                }
            }
        }
        Verdict::Failed(last)
    }

    fn retry<H: Host>(&self, host: &H, id: &str, message: String) {
        self.update(host, id, |progress| {
            progress.phase = Phase::Retrying;
            progress.message = Some(message);
            progress.speed = None;
            progress.eta_secs = None;
        });
    }

    /// Edit one snapshot, then publish it outside the lock.
    fn update<H: Host>(&self, host: &H, id: &str, edit: impl FnOnce(&mut Progress)) {
        {
            let Ok(mut slots) = self.slots.lock() else {
                return;
            };
            let Some(slot) = slots.get_mut(id) else {
                return;
            };
            edit(&mut slot.progress);
        }
        self.publish(host, id);
    }

    /// Read one snapshot, then publish it outside the lock.
    fn publish<H: Host>(&self, host: &H, id: &str) {
        let snapshot = self.status(id);
        if let Some(snapshot) = snapshot {
            self.emit(host, id, snapshot);
        }
    }

    fn emit<H: Host>(&self, host: &H, id: &str, snapshot: Progress) {
        host.publish(&snapshot);
        let hook = self
            .lock()
            .ok()
            .and_then(|slots| slots.get(id).and_then(|slot| slot.job.on_progress.clone()));
        if let Some(hook) = hook {
            hook(&snapshot);
        }
    }

    /// A stop that is not an ending: the record stays, and so do its bytes.
    fn settle<H: Host>(&self, host: &H, id: &str, phase: Phase) {
        {
            let Ok(mut slots) = self.slots.lock() else {
                return;
            };
            let Some(slot) = slots.get_mut(id) else {
                return;
            };
            slot.running = false;
            slot.progress.phase = phase;
            slot.progress.speed = None;
            slot.progress.eta_secs = None;
        }
        self.publish(host, id);
    }

    /// The ending a transport failure reports.
    fn failed<H: Host>(&self, host: &H, id: &str, message: String) {
        self.finish(host, id, Phase::Failed, Err(message));
    }

    /// Stamp the ending, keep it, and wake everyone waiting on this id.
    fn finish<H: Host>(&self, host: &H, id: &str, phase: Phase, result: Result<Outcome, String>) {
        let (snapshot, waiters) = {
            let Ok(mut slots) = self.slots.lock() else {
                return;
            };
            let Some(slot) = slots.get_mut(id) else {
                return;
            };
            slot.running = false;
            apply(&mut slot.progress, phase, &result);
            slot.outcome = Some(result.clone());
            (slot.progress.clone(), std::mem::take(&mut slot.waiters))
        };
        for waiter in waiters {
            let _ = waiter.send(result.clone());
        }
        self.emit(host, id, snapshot);
    }
}

/// What an ending says about the snapshot a frontend last saw.
fn apply(progress: &mut Progress, phase: Phase, result: &Result<Outcome, String>) {
    progress.phase = phase;
    progress.speed = None;
    progress.eta_secs = None;
    match result {
        Ok(outcome) => {
            progress.received = outcome.bytes;
            progress.total = Some(outcome.bytes);
            progress.path = Some(outcome.path.clone());
            progress.cached = outcome.cached;
            progress.message = None;
        }
        Err(message) => {
            progress.message = Some(message.clone());
        }
    }
}

/// Whole seconds left at `speed`, when both it and the total are known.
fn eta(received: u64, total: Option<u64>, speed: Option<f64>) -> Option<u64> {
    let speed = speed.filter(|speed| *speed > 1.0)?;
    let left = total?.saturating_sub(received);
    Some((left as f64 / speed) as u64)
}

/// The delay before an attempt repeats, doubling to a ceiling.
fn delay_for(attempt: u32) -> Duration {
    (BACKOFF_BASE * (1u32 << attempt.min(4))).min(BACKOFF_CEIL)
}

/// Sleep a backoff out in slices, so a stop lands during it.
async fn backoff(flags: &Flags, attempt: u32) {
    let mut left = delay_for(attempt);
    while left > Duration::ZERO && flags.stop().is_none() {
        let slice = left.min(BACKOFF_SLICE);
        tokio::time::sleep(slice).await;
        left -= slice;
    }
}

/// A stop with no phase of its own: the record went away mid-flight.
fn stopped(flags: &Flags) -> Verdict {
    Verdict::Stopped(flags.stop().unwrap_or(Phase::Cancelled))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host that keeps every snapshot instead of emitting one.
    #[derive(Clone, Default)]
    struct RecordingHost {
        seen: Arc<Mutex<Vec<Progress>>>,
    }

    impl Host for RecordingHost {
        fn spawn(&self, task: impl std::future::Future<Output = ()> + Send + 'static) {
            tokio::spawn(task);
        }

        fn publish(&self, progress: &Progress) {
            if let Ok(mut seen) = self.seen.lock() {
                seen.push(progress.clone());
            }
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dl_mgr_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_backoff_doubles_and_stops_growing() {
        assert_eq!(delay_for(1), Duration::from_millis(1000));
        assert_eq!(delay_for(2), Duration::from_millis(2000));
        assert_eq!(delay_for(3), Duration::from_millis(4000));
        assert_eq!(delay_for(99), BACKOFF_CEIL);
    }

    #[test]
    fn eta_needs_a_size_a_speed_and_a_real_one() {
        assert_eq!(eta(10, Some(110), Some(10.0)), Some(10));
        assert_eq!(eta(10, None, Some(10.0)), None);
        assert_eq!(eta(10, Some(110), None), None);
        // A crawl is not an estimate; a bar frozen at "9 years" is a lie.
        assert_eq!(eta(10, Some(110), Some(0.5)), None);
        // A body already past its declared size cannot go negative.
        assert_eq!(eta(200, Some(110), Some(10.0)), Some(0));
    }

    #[test]
    fn an_ending_rewrites_the_snapshot_a_frontend_last_saw() {
        let mut progress = Progress::new("x");
        progress.phase = Phase::Downloading;
        progress.speed = Some(1000.0);
        progress.eta_secs = Some(4);
        apply(
            &mut progress,
            Phase::Done,
            &Ok(Outcome {
                id: "x".into(),
                path: PathBuf::from("/tmp/x.bin"),
                bytes: 4096,
                source: 1,
                cached: false,
            }),
        );
        assert_eq!(progress.phase, Phase::Done);
        assert_eq!(progress.received, 4096);
        assert_eq!(progress.percent(), Some(100));
        assert_eq!(progress.path.as_deref(), Some(Path::new("/tmp/x.bin")));
        // A finished bar has no speed and nothing left to wait for.
        assert_eq!(progress.speed, None);
        assert_eq!(progress.eta_secs, None);

        apply(
            &mut progress,
            Phase::Failed,
            &Err("every mirror failed".into()),
        );
        assert_eq!(progress.phase, Phase::Failed);
        assert_eq!(progress.message.as_deref(), Some("every mirror failed"));

        // A cancel is an ending of its own, not a failure wearing its name.
        apply(&mut progress, Phase::Cancelled, &Err("cancelled".into()));
        assert_eq!(progress.phase, Phase::Cancelled);
        assert!(progress.phase.terminal());
    }

    #[tokio::test]
    async fn a_job_that_names_no_file_is_refused_before_anything_starts() {
        let registry = Downloads::new();
        let host = RecordingHost::default();
        let job = Job::new("bad", temp_dir("refused"), "https://host/data/");
        assert!(registry.start(&host, job).is_err());
        assert!(registry.status("bad").is_none());
        assert!(host.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_complete_file_on_disk_needs_no_network() {
        let dir = temp_dir("cached");
        let dest = dir.join("body.bin");
        std::fs::write(&dest, b"already here").unwrap();

        let registry = Downloads::new();
        let host = RecordingHost::default();
        // An unroutable source: reaching it would hang, not answer.
        let job = Job::new("cached", &dir, "http://127.0.0.1:9/never/body.bin").named("body.bin");
        let outcome = registry
            .start(&host, job)
            .unwrap()
            .finished()
            .await
            .unwrap();
        assert!(outcome.cached);
        assert_eq!(outcome.path, dest);
        assert_eq!(outcome.bytes, 12);
        assert_eq!(registry.status("cached").unwrap().phase, Phase::Done);
    }

    #[tokio::test]
    async fn a_file_that_fails_its_check_is_downloaded_again() {
        let dir = temp_dir("stale");
        let dest = dir.join("body.bin");
        std::fs::write(&dest, b"<html>Not Found</html>").unwrap();

        let registry = Downloads::new();
        let host = RecordingHost::default();
        let job = Job::new("stale", &dir, "http://127.0.0.1:9/x/body.bin")
            .named("body.bin")
            .verified_by(|path| {
                let head = std::fs::read(path).unwrap_or_default();
                if head.starts_with(b"<html") {
                    return Err("an error page, not a body".to_string());
                }
                Ok(())
            });
        // Every mirror is unroutable, so the check is what the failure names.
        let error = registry
            .start(&host, job)
            .unwrap()
            .finished()
            .await
            .unwrap_err();
        assert!(error.contains("connect"), "{error}");
        // The stale file is gone: it was not this download's to keep.
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn every_mirror_is_spent_before_the_download_fails() {
        let dir = temp_dir("mirrors");
        let registry = Downloads::new();
        let host = RecordingHost::default();
        let job = Job::new("mirrors", &dir, "http://127.0.0.1:9/a/body.bin")
            .mirror("http://127.0.0.1:9/b/body.bin")
            .named("body.bin");
        let error = registry
            .start(&host, job)
            .unwrap()
            .finished()
            .await
            .unwrap_err();
        assert!(error.contains("connect"), "{error}");
        assert_eq!(registry.status("mirrors").unwrap().phase, Phase::Failed);
        // Both mirrors were tried, and the last attempt of each was recorded.
        let seen = host.seen.lock().unwrap().clone();
        let mut sources: Vec<u32> = seen
            .iter()
            .filter_map(|progress| progress.source)
            .collect::<Vec<_>>();
        sources.dedup();
        assert!(sources.contains(&0) && sources.contains(&1), "{sources:?}");
    }

    #[tokio::test]
    async fn removing_a_record_wakes_its_waiter_and_drops_its_files() {
        let dir = temp_dir("removed");
        let registry = Downloads::new();
        let host = RecordingHost::default();
        let job = Job::new("removed", &dir, "http://127.0.0.1:9/a/body.bin").named("body.bin");
        let receipt = registry.start(&host, job).unwrap();
        registry.remove("removed");
        let error = receipt.finished().await.unwrap_err();
        assert!(error.contains("removed"), "{error}");
        assert!(registry.status("removed").is_none());
        assert!(registry.list().is_empty());
    }

    #[tokio::test]
    async fn a_running_id_refuses_a_second_transport() {
        let dir = temp_dir("twice");
        let registry = Downloads::new();
        let host = RecordingHost::default();
        let job = || Job::new("twice", &dir, "http://127.0.0.1:9/a/body.bin").named("body.bin");
        let receipt = registry.start(&host, job()).unwrap();
        let second = registry.start(&host, job());
        assert!(second.is_err(), "one transport per id");
        registry.cancel("twice");
        let _ = receipt.finished().await;
    }

    #[tokio::test]
    async fn the_registry_stays_bounded_by_finished_records_first() {
        let dir = temp_dir("cap");
        let registry = Downloads::new();
        let host = RecordingHost::default();
        for index in 0..(SLOT_CAP + 5) {
            let id = format!("job-{index}");
            // A file on disk finishes offline, so every record is terminal.
            std::fs::write(dir.join(format!("body-{index}.bin")), b"x").unwrap();
            let job =
                Job::new(&id, &dir, "http://127.0.0.1:9/a/b").named(format!("body-{index}.bin"));
            let _ = registry.start(&host, job).unwrap().finished().await;
        }
        assert!(
            registry.list().len() <= SLOT_CAP,
            "{}",
            registry.list().len()
        );
        // The newest record is the one that survives its own insertion.
        assert!(registry.status(&format!("job-{}", SLOT_CAP + 4)).is_some());
        assert!(registry.status("job-0").is_none());
    }

    #[tokio::test]
    async fn resume_refuses_an_id_that_never_paused() {
        let dir = temp_dir("resume");
        let registry = Downloads::new();
        let host = RecordingHost::default();
        assert!(registry.resume(&host, "absent").is_err());
        std::fs::write(dir.join("body.bin"), b"here").unwrap();
        let job = Job::new("resume", &dir, "http://127.0.0.1:9/a/b").named("body.bin");
        let _ = registry.start(&host, job).unwrap().finished().await;
        // A finished download is not a paused one.
        assert!(registry.resume(&host, "resume").is_err());
    }

    #[test]
    fn pausing_and_cancelling_an_absent_id_is_not_an_error() {
        let registry = Downloads::new();
        registry.pause("absent");
        registry.cancel("absent");
        registry.remove("absent");
        assert!(registry.status("absent").is_none());
        assert!(registry.outcome("absent").is_none());
    }
}
