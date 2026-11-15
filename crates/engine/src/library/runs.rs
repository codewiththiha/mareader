//! The long jobs the library has going, and how far each has got.
//!
//! A corner card shows one, but what a run is, the word its worker sends, and
//! how long it waits before it may show are the library's own; the front end
//! draws them and asks nothing back.

/// Seconds a run waits before its card shows.
pub const DELAY: f64 = 0.4;

/// Which run a card or a beat means.
pub type RunId = u64;

/// What a run is doing, which sets its headline and its ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Walking a folder: no total yet, so the ring spins.
    Scanning,
    /// Measuring what a walk found.
    Reading,
    Copying,
    /// The AI planning shelves; the count runs across both its stages.
    Shelving,
    /// The AI filing books on them.
    Filing,
    /// An AI run stopped partway; it can carry on.
    Paused,
    /// An AI run done, its plan not yet reviewed.
    Ready,
}

/// The worker's word on a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beat {
    pub run: RunId,
    pub phase: Phase,
    /// Files finished so far.
    pub done: usize,
    pub total: usize,
    /// The file or folder at hand.
    pub name: String,
}

impl Beat {
    /// Whether a beat at `done` of `total` moves the ring a whole percent.
    pub fn worth(done: usize, total: usize) -> bool {
        let percent = |n: usize| n * 100 / total.max(1);
        done == 0 || percent(done) != percent(done.saturating_sub(1))
    }
}

/// One job going, as the library keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub id: RunId,
    /// What was asked for: a file's name, "3 files", a folder's name.
    pub label: String,
    pub phase: Phase,
    pub done: usize,
    pub total: usize,
    pub name: String,
    /// When its card first could show, in seconds since the app started.
    started: Option<f64>,
}

impl Run {
    /// How far along, once there's a total.
    pub fn fraction(&self) -> Option<f32> {
        match (self.phase, self.total) {
            (Phase::Scanning, _) | (_, 0) => None,
            _ => Some((self.done as f32 / self.total as f32).clamp(0.0, 1.0)),
        }
    }

    /// What its card says, in words.
    pub fn headline(&self) -> String {
        let at = (self.done + 1).min(self.total);
        match (self.phase, self.total) {
            (Phase::Scanning, _) => "Scanning…".to_owned(),
            (Phase::Reading, 0) => "Reading…".to_owned(),
            (Phase::Copying, 0) => "Importing…".to_owned(),
            (Phase::Reading, n) => format!("Reading {at} of {n}"),
            (Phase::Copying, n) => format!("Importing {at} of {n}"),
            (Phase::Shelving, 0) => "Organizing…".to_owned(),
            (Phase::Shelving, _) => "Planning shelves…".to_owned(),
            (Phase::Filing, _) => "Filing books…".to_owned(),
            (Phase::Paused, _) => "Organizing paused".to_owned(),
            (Phase::Ready, _) => "Shelf plan ready".to_owned(),
        }
    }

    /// A stopped AI run, which the card resumes or lets go.
    pub fn paused(&self) -> bool {
        matches!(self.phase, Phase::Paused | Phase::Ready)
    }

    /// An AI run, which the card can stop.
    pub fn stoppable(&self) -> bool {
        matches!(self.phase, Phase::Shelving | Phase::Filing)
    }

    /// The file at hand, else what was asked for.
    pub fn detail(&self) -> &str {
        match self.name.is_empty() {
            true => &self.label,
            false => &self.name,
        }
    }

    /// Whether its card may show at `now`; the first look starts its wait.
    pub fn due(&mut self, now: f64) -> bool {
        let started = *self.started.get_or_insert(now);
        now - started >= DELAY
    }
}

/// The runs in flight, oldest first.
#[derive(Debug, Default)]
pub struct Runs {
    list: Vec<Run>,
    next: RunId,
}

impl Runs {
    /// A run begins under `label`; its id names it until it ends.
    pub fn start(&mut self, label: impl Into<String>, phase: Phase) -> RunId {
        self.next += 1;
        self.list.push(Run {
            id: self.next,
            label: label.into(),
            phase,
            done: 0,
            total: 0,
            name: String::new(),
            started: None,
        });
        self.next
    }

    /// Takes a worker's word, if the run it names is still going.
    pub fn beat(&mut self, beat: Beat) {
        if let Some(run) = self.list.iter_mut().find(|r| r.id == beat.run) {
            run.phase = beat.phase;
            run.done = beat.done;
            run.total = beat.total;
            run.name = beat.name;
        }
    }

    /// The run moved on to `phase`; its count starts again.
    pub fn phase(&mut self, id: Option<RunId>, phase: Phase) {
        if let Some(run) = self.list.iter_mut().find(|r| Some(r.id) == id) {
            run.phase = phase;
            run.done = 0;
            run.total = 0;
        }
    }

    /// The run ended; its toast says how.
    pub fn finish(&mut self, id: Option<RunId>) {
        self.list.retain(|r| Some(r.id) != id);
    }

    pub fn list(&self) -> &[Run] {
        &self.list
    }

    /// Each run, so the front end can ask one by one whether it is due.
    pub fn list_mut(&mut self) -> &mut [Run] {
        &mut self.list
    }

    /// A folder walk and an import, shown from the first frame, for shots.
    pub fn demo(&mut self) {
        self.start("Books", Phase::Scanning);
        let run = self.start("12 files", Phase::Copying);
        self.beat(Beat {
            run,
            phase: Phase::Copying,
            done: 4,
            total: 12,
            name: "Middlemarch.epub".into(),
        });
        self.show_now();
    }

    /// Every card shows from the first frame, for shots.
    pub fn show_now(&mut self) {
        for run in &mut self.list {
            run.started = Some(f64::NEG_INFINITY);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat(run: RunId, phase: Phase, done: usize, total: usize) -> Beat {
        Beat {
            run,
            phase,
            done,
            total,
            name: "ferns.epub".into(),
        }
    }

    #[test]
    fn a_scan_spins_then_reading_fills_the_ring() -> Result<(), &'static str> {
        let mut runs = Runs::default();
        let id = runs.start("Books", Phase::Scanning);
        let run = |r: &Runs| r.list().first().cloned().ok_or("a run was started");
        assert_eq!(run(&runs)?.fraction(), None);
        assert_eq!(run(&runs)?.headline(), "Scanning…");
        assert_eq!(run(&runs)?.detail(), "Books");
        runs.beat(beat(id, Phase::Reading, 3, 12));
        assert_eq!(run(&runs)?.fraction(), Some(0.25));
        assert_eq!(run(&runs)?.headline(), "Reading 4 of 12");
        assert_eq!(run(&runs)?.detail(), "ferns.epub");
        runs.beat(beat(id, Phase::Copying, 12, 12));
        assert_eq!(run(&runs)?.headline(), "Importing 12 of 12");
        runs.finish(Some(id));
        assert!(runs.list().is_empty());
        Ok(())
    }

    #[test]
    fn beats_for_other_runs_and_unknown_ends_change_nothing() -> Result<(), &'static str> {
        let mut runs = Runs::default();
        let a = runs.start("a.pdf", Phase::Copying);
        let b = runs.start("2 files", Phase::Copying);
        runs.beat(beat(b + 1, Phase::Copying, 1, 2));
        runs.finish(None);
        assert_eq!(runs.list().len(), 2);
        let head = runs.list().first().map(Run::headline);
        assert_eq!(head.ok_or("two runs are going")?, "Importing…");
        runs.finish(Some(a));
        let first = runs.list().first().map(|r| r.id);
        assert_eq!(first.ok_or("one run is left")?, b);
        Ok(())
    }

    #[test]
    fn an_ai_run_fills_one_ring_across_its_stages_and_can_stop() -> Result<(), &'static str> {
        let mut runs = Runs::default();
        let id = runs.start("5 books · Fast", Phase::Shelving);
        let run = |r: &Runs| r.list().first().cloned().ok_or("a run was started");
        assert_eq!(run(&runs)?.headline(), "Organizing…");
        assert!(run(&runs)?.stoppable());
        runs.beat(Beat {
            run: id,
            phase: Phase::Filing,
            done: 3,
            total: 4,
            name: String::new(),
        });
        assert_eq!(run(&runs)?.fraction(), Some(0.75));
        assert_eq!(run(&runs)?.headline(), "Filing books…");
        assert_eq!(run(&runs)?.detail(), "5 books · Fast");
        let import = runs.start("a.pdf", Phase::Copying);
        assert!(!runs.list().iter().any(|r| r.id == import && r.stoppable()));
        Ok(())
    }

    #[test]
    fn beats_are_sent_once_a_percent() {
        let sent = (0..1000).filter(|&d| Beat::worth(d, 1000)).count();
        assert_eq!(sent, 100);
        assert!(
            (0..7).all(|d| Beat::worth(d, 7)),
            "small runs beat every file"
        );
    }
}
