//! Voice models on disk, and downloads that run one at a time.

use std::collections::{HashMap, VecDeque};

use crate::models::{Build, MODELS, Model, ModelFile};

/// One download: a model's shared files, a build of it, or a voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    Model(&'static str),
    /// A model's id, then the build's; brings the shared files if missing.
    Build(&'static str, &'static str),
    /// A model's id, then the voice's.
    Voice(&'static str, &'static str),
}

impl Item {
    pub fn model(self) -> &'static str {
        match self {
            Self::Model(m) | Self::Build(m, _) | Self::Voice(m, _) => m,
        }
    }
}

/// Where a download stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Absent,
    /// Queued behind another download.
    Waiting,
    Fetching {
        got: u64,
        total: u64,
    },
    Paused {
        got: u64,
        total: u64,
    },
    /// Whole, its checksum being read.
    Checking,
    Ready,
    Failed(String),
}

impl Stage {
    /// Running or about to.
    pub fn busy(&self) -> bool {
        matches!(self, Self::Waiting | Self::Fetching { .. } | Self::Checking)
    }
}

const ABSENT: Stage = Stage::Absent;

/// The models on offer and the downloads in hand.
pub struct Shelf {
    catalogue: &'static [Model],
    /// Models another shelf keeps in the same folder, to share files with.
    neighbours: &'static [Model],
    stages: HashMap<Item, Stage>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    queue: VecDeque<Item>,
    #[cfg(not(target_arch = "wasm32"))]
    desk: Option<desk::Desk>,
    #[cfg(not(target_arch = "wasm32"))]
    studio: crate::studio::Studio,
}

impl Default for Shelf {
    fn default() -> Self {
        Self {
            catalogue: MODELS,
            neighbours: &[],
            stages: HashMap::new(),
            queue: VecDeque::new(),
            #[cfg(not(target_arch = "wasm32"))]
            desk: None,
            #[cfg(not(target_arch = "wasm32"))]
            studio: crate::studio::Studio::default(),
        }
    }
}

impl Shelf {
    #[cfg(not(target_arch = "wasm32"))]
    mareader_fetch::attached_method!();

    #[cfg(target_arch = "wasm32")]
    pub fn attached(&self) -> bool {
        false
    }

    pub fn catalogue(&self) -> &'static [Model] {
        self.catalogue
    }

    pub fn model(&self, id: &str) -> Option<&'static Model> {
        self.catalogue.iter().find(|m| m.id == id)
    }

    /// Shares files both ways with `other`; call once both have attached.
    pub fn share_with(&mut self, other: &mut Shelf) {
        self.neighbours = other.catalogue;
        other.neighbours = self.catalogue;
    }

    /// Every other model here or beside, which may hold the same files.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn others(&self, model: &Model) -> impl Iterator<Item = &'static Model> {
        let id = model.id;
        let all = self.catalogue.iter().chain(self.neighbours);
        all.filter(move |m| m.id != id)
    }

    /// Other models holding every file `model` needs, whole on disk.
    pub fn holders(&self, model: &Model) -> Vec<&'static Model> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(desk) = &self.desk {
            let files = model.core();
            let holds = |m: &&Model| {
                !files.is_empty() && files.iter().all(|f| twin_in(desk, m, f).is_some())
            };
            return self.others(model).filter(holds).collect();
        }
        let _ = model;
        Vec::new()
    }

    /// Takes in absent models whose files others hold, linked, not fetched.
    pub fn adopt(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        for model in self.catalogue {
            let item = Item::Model(model.id);
            if *self.stage(item) == Stage::Absent && !self.holders(model).is_empty() {
                self.download(item);
            }
        }
    }

    /// Keeps at most `bytes` of speech the models made, to say again.
    pub fn set_speech_memory(&self, bytes: usize) {
        #[cfg(not(target_arch = "wasm32"))]
        self.studio.set_memory(bytes);
        #[cfg(target_arch = "wasm32")]
        let _ = bytes;
    }

    /// Lets an unused model go after `idle`; `None` keeps it while reading.
    pub fn set_model_idle(&self, idle: Option<std::time::Duration>) {
        #[cfg(not(target_arch = "wasm32"))]
        self.studio.set_idle(idle);
        #[cfg(target_arch = "wasm32")]
        let _ = idle;
    }

    /// What the models hold in memory now.
    pub fn usage(&self) -> crate::Usage {
        #[cfg(not(target_arch = "wasm32"))]
        return self.studio.usage();
        #[cfg(target_arch = "wasm32")]
        crate::Usage::default()
    }

    pub fn stage(&self, item: Item) -> &Stage {
        self.stages.get(&item).unwrap_or(&ABSENT)
    }

    /// The models downloaded whole, with a build if they have builds.
    pub fn ready(&self) -> Vec<&'static Model> {
        let built = |m: &Model| m.builds.is_empty() || !self.builds(m).is_empty();
        let ready = |m: &&Model| self.shared(m) && built(m);
        self.catalogue.iter().filter(ready).collect()
    }

    /// Whether the files every build of `model` needs are here.
    fn shared(&self, model: &Model) -> bool {
        *self.stage(Item::Model(model.id)) == Stage::Ready
    }

    /// The builds of `model` downloaded, best first.
    pub fn builds(&self, model: &Model) -> Vec<&'static Build> {
        let here = |b: &&Build| *self.stage(Item::Build(model.id, b.id)) == Stage::Ready;
        model.builds.iter().filter(here).collect()
    }

    /// The build to run: `wanted` if here, else the best one here.
    pub fn build(&self, model: &Model, wanted: Option<&str>) -> Option<&'static Build> {
        let builds = self.builds(model);
        let wanted = builds.iter().find(|b| Some(b.id) == wanted);
        wanted.or(builds.first()).copied()
    }

    /// Whether `model` can speak in `voice` now.
    pub fn has(&self, model: &Model, voice: &str) -> bool {
        if !self.shared(model) {
            return false;
        }
        // A voice with no file of its own comes inside every build.
        let own = |v: &&crate::ModelVoice| {
            v.file.is_none() || *self.stage(Item::Voice(model.id, v.id)) == Stage::Ready
        };
        voice == model.first_voice || model.voice(voice).as_ref().is_some_and(own)
    }

    /// The files of `model`'s extra voices here, but for `except`'s.
    fn kept_by_voices(&self, model: &Model, except: Option<&str>) -> Vec<ModelFile> {
        let here = |v: &&crate::ModelVoice| {
            Some(v.id) != except
                && v.id != model.first_voice
                && *self.stage(Item::Voice(model.id, v.id)) == Stage::Ready
        };
        let mut files: Vec<ModelFile> = Vec::new();
        for f in model.voices.iter().filter(here).flat_map(|v| v.files()) {
            if !files.contains(&f) {
                files.push(f);
            }
        }
        files
    }

    /// What getting `voices` would fetch: files here or shared count once.
    pub fn to_fetch(&self, model: &Model, voices: &[&crate::ModelVoice]) -> u64 {
        let mut seen = self.kept_by_voices(model, None);
        let mut bytes = 0;
        for f in voices.iter().flat_map(|v| v.files()) {
            if !seen.contains(&f) {
                bytes += f.bytes;
                seen.push(f);
            }
        }
        bytes
    }

    /// All `model` keeps on disk, which removing it deletes.
    pub fn on_disk(&self, model: &Model) -> u64 {
        if !self.shared(model) {
            return 0;
        }
        let voices: u64 = self
            .kept_by_voices(model, None)
            .iter()
            .map(|f| f.bytes)
            .sum();
        // A file two builds share is on disk once.
        let mut files: Vec<ModelFile> = Vec::new();
        for f in self.builds(model).iter().flat_map(|b| b.files()) {
            if !files.contains(&f) {
                files.push(f);
            }
        }
        let builds = files.iter().map(|f| f.bytes).sum::<u64>();
        // Half-done downloads take room too, and go with the model.
        let partial: u64 = (self.stages.iter())
            .filter(|(item, _)| item.model() == model.id)
            .map(|(_, stage)| match stage {
                Stage::Fetching { got, .. } | Stage::Paused { got, .. } => *got,
                _ => 0,
            })
            .sum();
        model.bytes() + builds + voices + partial
    }

    /// Takes in what downloads sent; true if anything changed.
    pub fn poll(&mut self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(desk) = &mut self.desk else {
                return false;
            };
            let (events, ended) = desk.poll();
            let changed = !events.is_empty();
            for (item, stage) in events {
                // A first build brought the shared files with it.
                if let (Item::Build(m, _), Stage::Ready) = (item, &stage)
                    && let Some(model) = self.model(m)
                    && let Some(desk) = &self.desk
                {
                    let shared = desk.found(model, &model.core());
                    self.stages.insert(Item::Model(m), shared);
                }
                self.stages.insert(item, stage);
            }
            if let Some(model) = ended {
                self.forget(model);
            }
            self.pump();
            changed
        }
        #[cfg(target_arch = "wasm32")]
        false
    }

    /// Each item of `model`: shared files, each build, each extra voice.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn items(model: &'static Model) -> impl Iterator<Item = Item> {
        let builds = model.builds.iter().map(|b| Item::Build(model.id, b.id));
        let voices = model.voices.iter().filter(|v| v.id != model.first_voice);
        let voices = voices.filter(|v| v.file.is_some());
        let voices = voices.map(|v| Item::Voice(model.id, v.id));
        std::iter::once(Item::Model(model.id))
            .chain(builds)
            .chain(voices)
    }

    /// Every stage of `model` back to absent.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn forget(&mut self, model: &'static str) {
        self.stages.retain(|item, _| item.model() != model);
        self.queue.retain(|item| item.model() != model);
    }

    /// Starts the next waiting download if none runs.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn pump(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        while self.desk.as_ref().is_some_and(|d| d.current().is_none()) {
            let Some(item) = self.queue.pop_front() else {
                return;
            };
            let Some((model, files)) = self.files(item) else {
                continue;
            };
            let twins = self.twins(model, &files);
            if let Some(desk) = &mut self.desk {
                let got = desk.found(model, &files);
                let total = files.iter().map(|f| f.bytes).sum();
                let got = match got {
                    Stage::Paused { got, .. } => got,
                    _ => 0,
                };
                desk.start(item, model, files, twins);
                self.stages.insert(item, Stage::Fetching { got, total });
            }
        }
    }

    /// For each of `files`, the same file whole in another model's folder.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn twins(&self, model: &Model, files: &[ModelFile]) -> Vec<Option<std::path::PathBuf>> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(desk) = &self.desk {
            let others: Vec<&Model> = self.others(model).collect();
            let twin = |f: &ModelFile| others.iter().find_map(|m| twin_in(desk, m, f));
            return files.iter().map(twin).collect();
        }
        let _ = model;
        vec![None; files.len()]
    }

    /// What `item` downloads, and from which model.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn files(&self, item: Item) -> Option<(&'static Model, Vec<ModelFile>)> {
        let model = self.model(item.model())?;
        match item {
            Item::Model(_) => Some((model, model.core())),
            Item::Build(_, b) => {
                let build = model.build(b)?;
                // The first build comes with what every build shares.
                let mut files = match self.shared(model) {
                    true => Vec::new(),
                    false => model.core(),
                };
                // A file another build here has is that build's to keep.
                let others = self.builds(model).into_iter().filter(|o| o.id != b);
                let theirs: Vec<ModelFile> = others.flat_map(|o| o.files()).collect();
                files.extend(build.files().into_iter().filter(|f| !theirs.contains(f)));
                Some((model, files))
            }
            Item::Voice(_, v) => {
                let files = model.voice(v)?.files();
                if files.is_empty() {
                    return None;
                }
                // A file another voice here has is that voice's to keep.
                let theirs = self.kept_by_voices(model, Some(v));
                let own = files.into_iter().filter(|f| !theirs.contains(f));
                Some((model, own.collect()))
            }
        }
    }
}

/// `f` whole in `m`'s folder, under whatever name `m` lists it.
#[cfg(not(target_arch = "wasm32"))]
fn twin_in(desk: &desk::Desk, m: &Model, f: &ModelFile) -> Option<std::path::PathBuf> {
    let same = (m.files.iter()).find(|g| g.sha256 == f.sha256 && g.bytes == f.bytes)?;
    let path = desk.folder(m).join(same.path);
    let whole = std::fs::metadata(&path).is_ok_and(|d| d.len() == f.bytes);
    whole.then_some(path)
}

/// English pronunciations, loaded once for voices and word cards alike.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct Lexicons(crate::studio::Studio);

#[cfg(not(target_arch = "wasm32"))]
impl Lexicons {
    /// `dialect`'s phonemizer from the files in `dir`, unless held already.
    pub fn load(
        &self,
        dir: &std::path::Path,
        dialect: mareader_g2p::Dialect,
    ) -> Result<std::sync::Arc<mareader_g2p::G2p>, String> {
        let load = || mareader_g2p::G2p::load(dir, dialect).map_err(|e| e.to_string());
        self.0.g2p(dialect, load)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Shelf {
    /// What voices read pronunciations with, to share with others.
    pub fn lexicons(&self) -> Lexicons {
        Lexicons(self.studio.clone())
    }

    /// Keeps the models of `catalogue` in `dir`, made if missing.
    pub fn attach(
        &mut self,
        dir: std::path::PathBuf,
        catalogue: &'static [Model],
        waker: crate::Waker,
    ) -> Result<(), String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        self.catalogue = catalogue;
        self.stages.clear();
        self.queue.clear();
        let desk = desk::Desk::new(dir, waker);
        for model in catalogue {
            desk.retire(model);
            self.scan(&desk, model);
        }
        self.desk = Some(desk);
        Ok(())
    }

    /// Each item of `model` as found on disk.
    fn scan(&mut self, desk: &desk::Desk, model: &'static Model) {
        for item in Self::items(model) {
            if let Some((model, files)) = self.files(item) {
                self.stages.insert(item, desk.found(model, &files));
            }
        }
        // Once all are known, files one keeps leave the others.
        let kept = |i: &Item| matches!(i, Item::Build(..) | Item::Voice(..));
        for item in Self::items(model).filter(kept) {
            if *self.stage(item) != Stage::Ready
                && let Some((model, files)) = self.files(item)
            {
                self.stages.insert(item, desk.found(model, &files));
            }
        }
    }

    /// Queues `item`, or resumes it; a voice waits for its model.
    pub fn download(&mut self, item: Item) {
        if let Item::Voice(m, _) = item
            && *self.stage(Item::Model(m)) != Stage::Ready
        {
            return;
        }
        if self.files(item).is_none() || self.desk.is_none() {
            return;
        }
        match self.stage(item) {
            Stage::Absent | Stage::Paused { .. } | Stage::Failed(_) => {
                self.stages.insert(item, Stage::Waiting);
                self.queue.push_back(item);
                self.pump();
            }
            _ => {}
        }
    }

    /// Stops `item`, keeping what came to resume later.
    pub fn pause(&mut self, item: Item) {
        let Some(desk) = &mut self.desk else { return };
        if desk.current() == Some(item) {
            desk.halt(mareader_fetch::Halt::Pause);
        } else if *self.stage(item) == Stage::Waiting {
            self.queue.retain(|i| *i != item);
            if let Some((model, files)) = self.files(item)
                && let Some(desk) = &self.desk
            {
                self.stages.insert(item, desk.found(model, &files));
            }
        }
    }

    /// Stops `item` and drops what came of it.
    pub fn cancel(&mut self, item: Item) {
        let Some(desk) = &mut self.desk else { return };
        if desk.current() == Some(item) {
            desk.halt(mareader_fetch::Halt::Cancel);
            return;
        }
        self.queue.retain(|i| *i != item);
        if *self.stage(item) == Stage::Ready {
            return;
        }
        if let Some((model, files)) = self.files(item)
            && let Some(desk) = &self.desk
        {
            let _ = desk.drop_files(model, &files);
        }
        self.stages.insert(item, Stage::Absent);
    }

    /// Deletes `item`, or all for a last build; failing, keeps what's left.
    pub fn remove(&mut self, item: Item) -> Result<(), String> {
        let last = |s: &Self, m: &'static str| {
            let model = s.model(m);
            model.is_none_or(|model| s.builds(model).iter().all(|b| Item::Build(m, b.id) == item))
        };
        let item = match item {
            Item::Build(m, _) if last(self, m) => Item::Model(m),
            other => other,
        };
        let Some(desk) = &mut self.desk else {
            return Ok(());
        };
        let dropped = match item {
            Item::Model(m) => {
                let running = desk.current().is_some_and(|i| i.model() == m);
                // A running download's files go once it lets them go.
                let dropped = match running {
                    true => {
                        desk.doom(m);
                        Ok(())
                    }
                    false => desk.drop_model(m),
                };
                self.forget(m);
                dropped
            }
            Item::Build(..) | Item::Voice(..) => {
                if desk.current() == Some(item) {
                    desk.halt(mareader_fetch::Halt::Cancel);
                    return Ok(());
                }
                self.queue.retain(|i| *i != item);
                let dropped = match (self.files(item), &self.desk) {
                    (Some((model, files)), Some(desk)) => desk.drop_files(model, &files),
                    _ => Ok(()),
                };
                self.stages.insert(item, Stage::Absent);
                dropped
            }
        };
        let Err(why) = dropped else { return Ok(()) };
        let model = self.model(item.model());
        if let (Some(model), Some(desk)) = (model, self.desk.take()) {
            self.scan(&desk, model);
            self.desk = Some(desk);
        }
        let name = model.map_or(item.model(), |m| m.name);
        Err(format!("Couldn't delete all of {name}: {why}"))
    }

    /// The models loaded and the speech they made.
    pub(crate) fn studio(&self) -> &crate::studio::Studio {
        &self.studio
    }

    /// Where `model` is kept, once attached.
    pub fn folder(&self, model: &Model) -> Option<std::path::PathBuf> {
        self.desk.as_ref().map(|d| d.folder(model))
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod desk {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::{Duration, Instant};

    use mareader_fetch::{Halt, Kind, Stopped, fetch};

    use super::{Item, Stage};
    use crate::models::{Model, ModelFile};

    /// Progress is told this often at most.
    const TELL_EVERY: Duration = Duration::from_millis(100);

    type Event = (Item, Stage);

    pub struct Desk {
        dir: PathBuf,
        waker: crate::Waker,
        tell: Sender<Event>,
        events: Receiver<Event>,
        current: Option<(Item, Arc<AtomicU8>)>,
        /// A model to delete once its running download stops.
        doomed: Option<&'static str>,
    }

    impl Desk {
        pub fn new(dir: PathBuf, waker: crate::Waker) -> Self {
            let (tell, events) = channel();
            Self {
                dir,
                waker,
                tell,
                events,
                current: None,
                doomed: None,
            }
        }

        pub fn folder(&self, model: &Model) -> PathBuf {
            self.dir.join(model.id)
        }

        pub fn current(&self) -> Option<Item> {
            self.current.as_ref().map(|(item, _)| *item)
        }

        pub fn halt(&self, halt: Halt) {
            if let Some((_, flag)) = &self.current {
                flag.store(halt as u8, Ordering::Relaxed);
            }
        }

        /// Stops the running download; its model goes when it has.
        pub fn doom(&mut self, model: &'static str) {
            self.halt(Halt::Cancel);
            self.doomed = Some(model);
        }

        /// How much of `files` is on disk.
        pub fn found(&self, model: &Model, files: &[ModelFile]) -> Stage {
            found(&self.folder(model), files)
        }

        pub fn drop_files(&self, model: &Model, files: &[ModelFile]) -> Result<(), String> {
            drop_files(&self.folder(model), files).map_err(|e| e.to_string())
        }

        /// Deletes what `model` kept of voices not offered.
        pub fn retire(&self, model: &Model) {
            let folder = self.folder(model);
            let retired = crate::models::RETIRED.iter();
            for (_, path) in retired.filter(|(m, _)| *m == model.id) {
                let path = folder.join(path);
                let half = std::fs::remove_file(part(&path)).is_ok();
                if !(std::fs::remove_file(&path).is_ok() || half) {
                    continue;
                }
                // Folders left empty go too; a full one stays.
                let mut dir = path.parent();
                while let Some(d) = dir.filter(|d| *d != folder) {
                    if std::fs::remove_dir(d).is_err() {
                        break;
                    }
                    dir = d.parent();
                }
            }
        }

        /// Deletes `model`'s folder; one already gone counts as deleted.
        pub fn drop_model(&self, model: &str) -> Result<(), String> {
            gone(std::fs::remove_dir_all(self.dir.join(model))).map_err(|e| e.to_string())
        }

        /// What came, and a doomed model's id once deleted.
        pub fn poll(&mut self) -> (Vec<Event>, Option<&'static str>) {
            let mut events: Vec<Event> = self.events.try_iter().collect();
            let ended = events.iter().any(|(item, stage)| {
                self.current() == Some(*item)
                    && !matches!(stage, Stage::Fetching { .. } | Stage::Checking)
            });
            // A doomed model's news is stale: it is already forgotten.
            if let Some(model) = self.doomed {
                events.retain(|(item, _)| item.model() != model);
            }
            if !ended {
                return (events, None);
            }
            self.current = None;
            let doomed = self.doomed.take();
            if let Some(model) = doomed {
                let _ = self.drop_model(model);
            }
            (events, doomed)
        }

        /// Fetches `files` on a thread; a file with a twin is linked instead.
        pub fn start(
            &mut self,
            item: Item,
            model: &'static Model,
            files: Vec<ModelFile>,
            twins: Vec<Option<PathBuf>>,
        ) {
            let flag = Arc::new(AtomicU8::new(Halt::Go as u8));
            self.current = Some((item, Arc::clone(&flag)));
            let folder = self.folder(model);
            let (tell, waker) = (self.tell.clone(), Arc::clone(&self.waker));
            std::thread::spawn(move || {
                let say = |stage: Stage| {
                    let _ = tell.send((item, stage));
                    waker();
                };
                let stage = run(model, (&folder, &files, &twins), &flag, &say);
                say(stage);
            });
        }
    }

    /// Each file not yet whole, in turn; the stage it ends at.
    fn run(
        model: &Model,
        (folder, files, twins): (&Path, &[ModelFile], &[Option<PathBuf>]),
        flag: &AtomicU8,
        say: &dyn Fn(Stage),
    ) -> Stage {
        let total: u64 = files.iter().map(|f| f.bytes).sum();
        let mut base = 0;
        let now = Instant::now();
        let mut last = now.checked_sub(TELL_EVERY).unwrap_or(now);
        for (i, f) in files.iter().enumerate() {
            let path = folder.join(f.path);
            if len(&path) == Some(f.bytes) {
                base += f.bytes;
                continue;
            }
            if let Some(dir) = path.parent()
                && let Err(e) = std::fs::create_dir_all(dir)
            {
                return Stage::Failed(e.to_string());
            }
            let part = part(&path);
            let twin = twins.get(i).and_then(Option::as_deref);
            if let Some(twin) = twin
                && link(twin, &part, f.bytes)
                && std::fs::rename(&part, &path).is_ok()
            {
                base += f.bytes;
                continue;
            }
            let kind = Kind::Sha256 {
                sha256: f.sha256,
                bytes: f.bytes,
            };
            let mut progress = |got: u64, _| {
                let got = base + got;
                if got >= total {
                    say(Stage::Checking);
                } else if last.elapsed() >= TELL_EVERY {
                    last = Instant::now();
                    say(Stage::Fetching { got, total });
                }
            };
            match fetch(&model.urls(f), &part, kind, flag, &mut progress) {
                Ok(()) => {
                    if let Err(e) = std::fs::rename(&part, &path) {
                        return Stage::Failed(e.to_string());
                    }
                }
                Err(Stopped::Halted(Halt::Pause)) => {
                    let got = base + len(&part).unwrap_or(0);
                    return Stage::Paused { got, total };
                }
                Err(Stopped::Halted(_)) => {
                    let _ = drop_files(folder, files);
                    return Stage::Absent;
                }
                Err(Stopped::Failed(why)) => return Stage::Failed(why),
            }
            base += f.bytes;
        }
        Stage::Ready
    }

    /// Whole when every file is its listed size; else what is here.
    fn found(folder: &Path, files: &[ModelFile]) -> Stage {
        let total: u64 = files.iter().map(|f| f.bytes).sum();
        let (mut got, mut whole, mut any) = (0, true, false);
        for f in files {
            let path = folder.join(f.path);
            if len(&path) == Some(f.bytes) {
                (got, any) = (got + f.bytes, true);
                continue;
            }
            whole = false;
            if let Some(n) = len(&part(&path)) {
                (got, any) = (got + n, true);
            }
        }
        match (whole, any) {
            (true, _) => Stage::Ready,
            (false, true) => Stage::Paused { got, total },
            (false, false) => Stage::Absent,
        }
    }

    /// Deletes `files` and their parts, trying each; the first error wins.
    fn drop_files(folder: &Path, files: &[ModelFile]) -> std::io::Result<()> {
        let mut first = Ok(());
        for f in files {
            let path = folder.join(f.path);
            for done in [
                std::fs::remove_file(part(&path)),
                std::fs::remove_file(path),
            ] {
                if first.is_ok() {
                    first = gone(done);
                }
            }
        }
        first
    }

    /// A delete that found nothing left to delete still worked.
    fn gone(done: std::io::Result<()>) -> std::io::Result<()> {
        match done {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    /// `to` made the same file as `from`, shared on disk where it can be.
    fn link(from: &Path, to: &Path, bytes: u64) -> bool {
        let _ = std::fs::remove_file(to);
        let made = std::fs::hard_link(from, to).is_ok() || std::fs::copy(from, to).is_ok();
        made && len(to) == Some(bytes)
    }

    fn len(path: &Path) -> Option<u64> {
        std::fs::metadata(path).ok().map(|m| m.len())
    }

    /// `<path>.part`: where a file grows until it checks out.
    fn part(path: &Path) -> PathBuf {
        let mut name = path.as_os_str().to_owned();
        name.push(".part");
        PathBuf::from(name)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::time::Duration;

    use sha2::{Digest, Sha256};

    use super::*;
    use crate::models::ModelVoice;

    fn leak(s: String) -> &'static str {
        Box::leak(s.into_boxed_str())
    }

    fn body(n: usize, seed: u8) -> Vec<u8> {
        (0..n)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    fn sha(b: &[u8]) -> &'static str {
        leak(
            Sha256::digest(b)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        )
    }

    /// Serves `files` by path, ranges honoured; else 404.
    fn serve(files: HashMap<String, Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split(' ').nth(1).unwrap_or("/").to_owned();
                let mut from = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap() <= 2 {
                        break;
                    }
                    if let Some(r) = h.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        from = r.trim().trim_end_matches('-').parse().unwrap();
                    }
                }
                let (status, bytes, extra) = match files.get(&path) {
                    Some(full) if from > 0 => {
                        let range = format!(
                            "Content-Range: bytes {from}-{}/{}\r\n",
                            full.len() - 1,
                            full.len()
                        );
                        ("206 Partial Content", full[from..].to_vec(), range)
                    }
                    Some(full) => ("200 OK", full.clone(), String::new()),
                    None => ("404 Not Found", b"no".to_vec(), String::new()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                    bytes.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&bytes);
            }
        });
        format!("http://{addr}")
    }

    /// Models `ids`, each with a config, two builds and two voices.
    fn catalogue(ids: &[&str], wrong: bool) -> (&'static [Model], HashMap<String, Vec<u8>>) {
        let mut files = HashMap::new();
        let mut models = Vec::new();
        let mut listed = |id: &str, path: &str, n: usize, seed: u8| {
            let b = body(n, seed);
            let file = ModelFile {
                path: leak(path.to_owned()),
                bytes: n as u64,
                sha256: if wrong { sha(b"other") } else { sha(&b) },
                hosts: None,
                from: None,
            };
            files.insert(format!("/{id}/{path}"), b);
            file
        };
        let mut specs = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            let seed = i as u8 * 7;
            let config = listed(id, "config.json", 40, seed);
            let weights = listed(id, "weights.bin", 300_000, seed + 1);
            let lite = listed(id, "lite.bin", 100_000, seed + 4);
            let a = listed(id, "voices/a.pt", 2_000, seed + 2);
            let b = listed(id, "voices/b.pt", 3_000, seed + 3);
            // Served for tests that add a voice and a file voices share.
            listed(id, "voices/c.pt", 1_000, seed + 5);
            listed(id, "words.txt", 5_000, seed + 6);
            specs.push((leak((*id).to_owned()), config, [weights, lite], [a, b]));
        }
        let host = serve(files.clone());
        for (id, config, [weights, lite], [a, b]) in specs {
            let build = |bid: &'static str, file| Build {
                id: bid,
                name: bid,
                about: "A test build",
                file,
                extra: None,
            };
            let voice = |vid: &'static str, file| ModelVoice {
                id: vid,
                name: vid,
                language: "en-US",
                gender: crate::models::Gender::Female,
                file: Some(file),
                extra: None,
                shared: &[],
                note: "",
            };
            let hosts: &'static [&'static str] =
                Box::leak(vec![leak(format!("{host}/{id}/"))].into_boxed_slice());
            models.push(Model {
                id,
                name: id,
                about: "A test model",
                license: "MIT",
                hosts,
                files: Box::leak(vec![config].into_boxed_slice()),
                builds: Box::leak(
                    vec![build("big", weights), build("lite", lite)].into_boxed_slice(),
                ),
                voices: Box::leak(vec![voice("a", a), voice("b", b)].into_boxed_slice()),
                first_voice: "a",
                knobs: &[],
            });
        }
        (Box::leak(models.into_boxed_slice()), files)
    }

    fn shelf(dir: &std::path::Path, catalogue: &'static [Model]) -> Shelf {
        let mut shelf = Shelf::default();
        shelf
            .attach(dir.to_owned(), catalogue, Arc::new(|| {}))
            .unwrap();
        shelf
    }

    fn settle(shelf: &mut Shelf, until: impl Fn(&Shelf) -> bool) {
        for _ in 0..1000 {
            shelf.poll();
            if until(shelf) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("never settled: {:?}", shelf.stages);
    }

    #[test]
    fn files_of_a_retired_voice_go_and_their_empty_folders_with_them() {
        let dir = tempfile::tempdir().unwrap();
        let piper = dir.path().join("piper");
        let joe = piper.join("en/en_US/joe/medium");
        let john = piper.join("en/en_US/john/medium");
        for d in [&joe, &john] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(joe.join("en_US-joe-medium.onnx"), b"w").unwrap();
        std::fs::write(joe.join("en_US-joe-medium.onnx.json.part"), b"s").unwrap();
        std::fs::write(john.join("en_US-john-medium.onnx"), b"w").unwrap();
        let _ = shelf(dir.path(), MODELS);
        assert!(!piper.join("en/en_US/joe").exists());
        assert!(john.join("en_US-john-medium.onnx").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_delete_that_fails_says_why_and_shows_what_is_left() {
        use std::fs::{Permissions, set_permissions};
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (models, _) = catalogue(&["tiny"], false);
        let mut s = shelf(dir.path(), models);
        let model = &models[0];
        s.download(Item::Build("tiny", "big"));
        settle(&mut s, |s| {
            *s.stage(Item::Build("tiny", "big")) == Stage::Ready
        });
        s.download(Item::Voice("tiny", "b"));
        settle(&mut s, |s| s.has(model, "b"));
        let voices = dir.path().join("tiny/voices");
        set_permissions(&voices, Permissions::from_mode(0o555)).unwrap();
        let removed = s.remove(Item::Voice("tiny", "b"));
        set_permissions(&voices, Permissions::from_mode(0o755)).unwrap();
        // Root deletes anyway, leaving nothing to check.
        if !voices.join("b.pt").exists() {
            return;
        }
        let why = removed.unwrap_err();
        assert!(why.starts_with("Couldn't delete all of"), "{why}");
        assert!(s.has(model, "b"));
        // Already gone by other means, it still deletes.
        std::fs::remove_file(voices.join("b.pt")).unwrap();
        s.remove(Item::Voice("tiny", "b")).unwrap();
        assert!(!s.has(model, "b"));
    }

    #[test]
    fn a_model_comes_with_a_voice_takes_more_and_goes_whole() {
        let dir = tempfile::tempdir().unwrap();
        let (models, _) = catalogue(&["tiny"], false);
        let mut s = shelf(dir.path(), models);
        let model = &models[0];
        let (big, lite) = (Item::Build("tiny", "big"), Item::Build("tiny", "lite"));
        assert_eq!(*s.stage(Item::Voice("tiny", "b")), Stage::Absent);
        // A voice waits for its model.
        s.download(Item::Voice("tiny", "b"));
        assert_eq!(*s.stage(Item::Voice("tiny", "b")), Stage::Absent);
        // The first build brings the shared files and first voice.
        s.download(big);
        assert!(s.stage(big).busy());
        settle(&mut s, |s| *s.stage(big) == Stage::Ready);
        assert_eq!(*s.stage(Item::Model("tiny")), Stage::Ready);
        assert!(s.has(model, "a") && !s.has(model, "b"));
        assert_eq!(s.ready().len(), 1);
        let folder = dir.path().join("tiny");
        assert!(folder.join("weights.bin").exists() && folder.join("voices/a.pt").exists());
        assert!(!folder.join("lite.bin").exists());
        s.download(Item::Voice("tiny", "b"));
        settle(&mut s, |s| s.has(model, "b"));
        assert_eq!(s.on_disk(model), 40 + 300_000 + 2_000 + 3_000);
        s.remove(Item::Voice("tiny", "b")).unwrap();
        assert!(!s.has(model, "b") && !folder.join("voices/b.pt").exists());
        // A second build shares the rest.
        s.download(lite);
        settle(&mut s, |s| *s.stage(lite) == Stage::Ready);
        assert_eq!(s.builds(model).len(), 2);
        assert_eq!(s.on_disk(model), 40 + 300_000 + 100_000 + 2_000);
        // A paused voice's part is on disk too, and goes with the model.
        let paused = Stage::Paused {
            got: 1_000,
            total: 3_000,
        };
        s.stages.insert(Item::Voice("tiny", "b"), paused);
        assert_eq!(s.on_disk(model), 40 + 300_000 + 100_000 + 2_000 + 1_000);
        s.stages.remove(&Item::Voice("tiny", "b"));
        assert_eq!(s.build(model, Some("lite")).map(|b| b.id), Some("lite"));
        assert_eq!(s.build(model, Some("gone")).map(|b| b.id), Some("big"));
        s.remove(big).unwrap();
        assert!(!folder.join("weights.bin").exists() && folder.join("lite.bin").exists());
        assert_eq!(s.build(model, Some("big")).map(|b| b.id), Some("lite"));
        // The last build takes the model with it.
        s.remove(lite).unwrap();
        assert!(!folder.exists());
        assert!(s.ready().is_empty());
        // Found again on the next start, by what is on disk.
        s.download(lite);
        settle(&mut s, |s| *s.stage(lite) == Stage::Ready);
        let again = shelf(dir.path(), models);
        assert_eq!(*again.stage(Item::Model("tiny")), Stage::Ready);
        assert_eq!(*again.stage(lite), Stage::Ready);
        assert_eq!(*again.stage(big), Stage::Absent);
    }

    #[test]
    fn builds_share_a_file_and_voices_inside_them_need_no_download() {
        let dir = tempfile::tempdir().unwrap();
        let (models, _) = catalogue(&["kit"], false);
        let one = models[0];
        let shared = one.voices[1].file.unwrap();
        let with = |b: Build| Build {
            extra: Some(shared),
            ..b
        };
        let inside = ModelVoice {
            file: None,
            ..one.voices[1]
        };
        let kit = Model {
            builds: Box::leak(vec![with(one.builds[0]), with(one.builds[1])].into_boxed_slice()),
            voices: Box::leak(vec![one.voices[0], inside].into_boxed_slice()),
            ..one
        };
        let all: &'static [Model] = Box::leak(vec![kit].into_boxed_slice());
        let model = &all[0];
        let mut s = shelf(dir.path(), all);
        assert!(Shelf::items(model).all(|i| i != Item::Voice("kit", "b")));
        let (big, lite) = (Item::Build("kit", "big"), Item::Build("kit", "lite"));
        s.download(big);
        settle(&mut s, |s| *s.stage(big) == Stage::Ready);
        assert!(s.has(model, "b"), "it came inside the build");
        s.download(Item::Voice("kit", "b"));
        assert_eq!(*s.stage(Item::Voice("kit", "b")), Stage::Absent);
        s.download(lite);
        settle(&mut s, |s| *s.stage(lite) == Stage::Ready);
        assert_eq!(s.on_disk(model), 40 + 2_000 + 300_000 + 100_000 + 3_000);
        let folder = dir.path().join("kit");
        s.remove(big).unwrap();
        assert!(!folder.join("weights.bin").exists());
        assert!(
            folder.join("voices/b.pt").exists(),
            "the other build keeps it"
        );
        let again = shelf(dir.path(), all);
        assert_eq!(*again.stage(lite), Stage::Ready);
        assert_eq!(*again.stage(big), Stage::Absent);
    }

    #[test]
    fn voices_share_a_file_that_stays_while_one_needs_it() {
        let dir = tempfile::tempdir().unwrap();
        let (models, _) = catalogue(&["zh"], false);
        let one = models[0];
        let made = |path: &'static str, n: usize, seed: u8| ModelFile {
            path,
            bytes: n as u64,
            sha256: sha(&body(n, seed)),
            hosts: None,
            from: None,
        };
        let words: &'static [ModelFile] = Box::leak(Box::new([made("words.txt", 5_000, 6)]));
        let b = ModelVoice {
            shared: words,
            ..one.voices[1]
        };
        let c = ModelVoice {
            id: "c",
            file: Some(made("voices/c.pt", 1_000, 5)),
            ..b
        };
        let zh = Model {
            voices: Box::leak(vec![one.voices[0], b, c].into_boxed_slice()),
            ..one
        };
        let all: &'static [Model] = Box::leak(vec![zh].into_boxed_slice());
        let model = &all[0];
        let mut s = shelf(dir.path(), all);
        s.download(Item::Build("zh", "lite"));
        settle(&mut s, |s| s.ready().len() == 1);
        let (vb, vc) = (Item::Voice("zh", "b"), Item::Voice("zh", "c"));
        assert_eq!(s.to_fetch(model, &[&b, &c]), 3_000 + 1_000 + 5_000);
        s.download(vb);
        settle(&mut s, |s| s.has(model, "b"));
        // The second voice fetches only its own file.
        assert_eq!(s.to_fetch(model, &[&c]), 1_000);
        s.download(vc);
        settle(&mut s, |s| s.has(model, "c"));
        assert_eq!(
            s.on_disk(model),
            40 + 100_000 + 2_000 + 3_000 + 1_000 + 5_000
        );
        let folder = dir.path().join("zh");
        s.remove(vb).unwrap();
        assert!(!folder.join("voices/b.pt").exists());
        assert!(folder.join("words.txt").exists(), "c still needs it");
        assert!(s.has(model, "c") && !s.has(model, "b"));
        // Found again on the next start, by what is on disk.
        let again = shelf(dir.path(), all);
        assert_eq!(*again.stage(vc), Stage::Ready);
        assert_eq!(*again.stage(vb), Stage::Absent);
        s.remove(vc).unwrap();
        assert!(!folder.join("words.txt").exists(), "the last one takes it");
    }

    #[test]
    fn a_model_without_builds_is_ready_alone_and_links_files_another_has() {
        let dir = tempfile::tempdir().unwrap();
        let (models, _) = catalogue(&["one"], false);
        let one = models[0];
        let (a, b) = (one.voices[0], one.voices[1]);
        let here = |f: ModelFile| ModelFile {
            hosts: Some(one.hosts),
            ..f
        };
        // Its own host has nothing: the shared config can only be linked.
        let bare = Model {
            id: "bare",
            hosts: &["http://127.0.0.1:9/"],
            builds: &[],
            voices: Box::leak(Box::new([ModelVoice {
                file: a.file.map(here),
                extra: b.file.map(here),
                ..a
            }])),
            ..one
        };
        let all: &'static [Model] = Box::leak(vec![one, bare].into_boxed_slice());
        let mut s = shelf(dir.path(), all);
        s.download(Item::Build("one", "big"));
        settle(&mut s, |s| s.ready().len() == 1);
        s.download(Item::Model("bare"));
        settle(&mut s, |s| !s.stage(Item::Model("bare")).busy());
        assert_eq!(*s.stage(Item::Model("bare")), Stage::Ready);
        assert_eq!(s.ready().len(), 2, "no build needed");
        assert!(s.has(&all[1], "a"));
        assert_eq!(s.on_disk(&all[1]), 40 + 2_000 + 3_000);
        let shared = dir.path().join("bare/config.json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let inode = |p: std::path::PathBuf| std::fs::metadata(p).unwrap().ino();
            assert_eq!(
                inode(shared.clone()),
                inode(dir.path().join("one/config.json"))
            );
        }
        // Each keeps its copy when the other goes.
        s.remove(Item::Model("one")).unwrap();
        assert!(shared.exists());
        assert_eq!(s.ready().len(), 1);
    }

    #[test]
    fn a_neighbour_shelf_adopts_files_a_model_holds_and_lends_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let (models, _) = catalogue(&["one"], false);
        let one = models[0];
        // Its own host has nothing: the config can only be linked.
        let aid = Model {
            id: "aid",
            hosts: &["http://127.0.0.1:9/"],
            builds: &[],
            voices: &[],
            first_voice: "",
            ..one
        };
        let aids: &'static [Model] = Box::leak(vec![aid].into_boxed_slice());
        let mut voices = shelf(dir.path(), models);
        let mut extra = shelf(dir.path(), aids);
        extra.share_with(&mut voices);
        extra.adopt();
        assert_eq!(*extra.stage(Item::Model("aid")), Stage::Absent);
        voices.download(Item::Build("one", "big"));
        settle(&mut voices, |s| s.ready().len() == 1);
        let names = |s: &Shelf| s.holders(&aids[0]).iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(names(&extra), ["one"]);
        extra.adopt();
        settle(&mut extra, |s| !s.stage(Item::Model("aid")).busy());
        assert_eq!(*extra.stage(Item::Model("aid")), Stage::Ready);
        let kept = dir.path().join("aid/config.json");
        assert!(kept.exists());
        // The model goes; the aid keeps its link and holds it alone.
        voices.remove(Item::Model("one")).unwrap();
        assert!(kept.exists());
        assert!(names(&extra).is_empty());
        // Back again, the model takes the aid's file rather than a copy.
        voices.download(Item::Build("one", "big"));
        settle(&mut voices, |s| s.ready().len() == 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let inode = |p: std::path::PathBuf| std::fs::metadata(p).unwrap().ino();
            let config = dir.path().join("one/config.json");
            assert_eq!(inode(kept), inode(config));
        }
    }

    #[test]
    fn downloads_take_turns_and_a_waiting_one_can_be_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (models, _) = catalogue(&["one", "two", "three"], false);
        let mut s = shelf(dir.path(), models);
        let big = |id| Item::Build(id, "big");
        for id in ["one", "two", "three"] {
            s.download(big(id));
        }
        assert!(matches!(s.stage(big("one")), Stage::Fetching { .. }));
        assert_eq!(*s.stage(big("two")), Stage::Waiting);
        s.cancel(big("three"));
        assert_eq!(*s.stage(big("three")), Stage::Absent);
        settle(&mut s, |s| *s.stage(big("two")) == Stage::Ready);
        assert_eq!(*s.stage(big("one")), Stage::Ready);
        assert_eq!(*s.stage(big("three")), Stage::Absent);
        assert!(!dir.path().join("three").exists());
    }

    #[test]
    fn a_file_with_the_wrong_checksum_is_never_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (models, _) = catalogue(&["bad"], true);
        let mut s = shelf(dir.path(), models);
        let item = Item::Build("bad", "big");
        s.download(item);
        settle(&mut s, |s| matches!(s.stage(item), Stage::Failed(_)));
        let why = Stage::Failed("the file isn't the one listed".into());
        assert_eq!(*s.stage(item), why);
        assert!(!dir.path().join("bad/config.json").exists());
    }

    #[test]
    fn a_stopped_download_is_found_paused_and_resumes() {
        let dir = tempfile::tempdir().unwrap();
        let (models, files) = catalogue(&["half"], false);
        let folder = dir.path().join("half");
        std::fs::create_dir_all(&folder).unwrap();
        let weights = &files["/half/weights.bin"];
        std::fs::write(folder.join("weights.bin.part"), &weights[..100_000]).unwrap();
        let mut s = shelf(dir.path(), models);
        let item = Item::Build("half", "big");
        let total = models[0].bytes_with(&models[0].builds[0]);
        let paused = Stage::Paused {
            got: 100_000,
            total,
        };
        assert_eq!(*s.stage(item), paused);
        s.download(item);
        settle(&mut s, |s| *s.stage(item) == Stage::Ready);
        assert_eq!(std::fs::read(folder.join("weights.bin")).unwrap(), *weights);
        assert!(!folder.join("weights.bin.part").exists());
    }
}
