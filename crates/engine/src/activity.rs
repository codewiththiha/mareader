//! The AI's work going on, as the corner stack lists it.
//!
//! Each owner keeps its own jobs; [`of_study`] and its kin read what each is
//! doing into one list a frame, so the window says it all in one place. A
//! job's [`Key`] stays the same from its start to its end, and the
//! [`Ledger`] keeps how each ended once its owner forgets it. See
//! `docs/study-mode.md` § The corner stack.

mod ledger;

pub use ledger::{End, LINGER, Ledger, News, Row};

use mareader_settings::TaskId;
use mareader_study::mode::{Batch, Via};
use mareader_workspace::PaneId;

use crate::study_mode::{Ahead, DeckJob, DeckState, Doing, Run, State};

/// Whose work it is: the whole app's, or one pane's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    Global,
    Pane(PaneId),
}

/// One job, the same across frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// A studying pane's deck, by its batch's first page.
    Cards { pane: PaneId, from: usize },
}

impl Key {
    pub fn scope(self) -> Scope {
        match self {
            Self::Cards { pane, .. } => Scope::Pane(pane),
        }
    }

    /// Whether stopping it asks first: a deck goes only with study mode.
    pub fn asks_to_stop(self) -> bool {
        matches!(self, Self::Cards { .. })
    }
}

/// Where a job stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    /// Waiting in line for its model.
    Queued,
    /// At work, with the share done, 0 to 1, when it is known.
    Running(Option<f32>),
    Failed,
}

/// A job, in the words the stack shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    pub key: Key,
    pub task: TaskId,
    /// What it does now: "Writing cards 3 of 8 · pages 4–6".
    pub said: String,
    /// The pages it works on: "pages 4–6".
    pub range: String,
    /// The page View goes to.
    pub page: usize,
    /// Whether, once done, it waits to be seen; else it says so a moment.
    pub keeps: bool,
    pub stage: Stage,
}

impl Activity {
    /// The words once it landed: "Cards ready · pages 4–6".
    pub fn done_said(&self) -> String {
        match (self.key, self.keeps) {
            (Key::Cards { .. }, true) => format!("Cards ready · {}", self.range),
            (Key::Cards { .. }, false) => "Next cards ready".to_owned(),
        }
    }

    /// The toast once it failed.
    pub fn failed_said(&self) -> String {
        match self.key {
            Key::Cards { .. } => format!("Couldn’t write the cards for {}", self.range),
        }
    }

    /// The toast once it was stopped.
    pub fn stopped_said(&self) -> String {
        match self.key {
            Key::Cards { .. } => format!("Stopped writing the cards for {}", self.range),
        }
    }
}

/// The AI's work on every studying pane's cards, each pane's gate first.
pub fn of_study(state: &State) -> Vec<Activity> {
    state
        .runs()
        .flat_map(|(pane, run)| of_run(pane, run))
        .collect()
}

/// Whether the job `key` names ended by landing; else it was stopped.
pub fn landed(state: &State, key: Key) -> bool {
    match key {
        // A deck's job ends in its cards, unless study mode went off.
        Key::Cards { pane, .. } => state.on(pane),
    }
}

fn of_run(pane: PaneId, run: &Run) -> Vec<Activity> {
    let mut all = Vec::new();
    match &run.deck {
        DeckState::Making { batch, job } => all.extend(making(pane, batch, job, true)),
        DeckState::Failed { batch, .. } => all.push(failed(pane, batch)),
        DeckState::None | DeckState::Open(_) => {}
    }
    for ahead in &run.ahead {
        match ahead {
            Ahead::Making { batch, job } => all.extend(making(pane, batch, job, false)),
            Ahead::Failed { batch, .. } => all.push(failed(pane, batch)),
            Ahead::Ready { .. } => {}
        }
    }
    all
}

/// A deck the AI writes, the gate's when `keeps`; a dictionary's come at
/// once and aren't listed.
fn making(pane: PaneId, batch: &Batch, job: &DeckJob, keeps: bool) -> Option<Activity> {
    if job.via() != Via::Ai {
        return None;
    }
    let range = batch.label();
    let made = Some(job.made());
    let (said, stage) = match (job.doing(), job.progress()) {
        _ if job.queued() => (format!("Waiting for the model · {range}"), Stage::Queued),
        (Doing::Writing, Some((done, of))) => (
            format!("Writing cards {done} of {of} · {range}"),
            Stage::Running(made),
        ),
        _ => (format!("Finding new words · {range}"), Stage::Running(made)),
    };
    let card = cards(pane, batch, range);
    Some(Activity {
        said,
        keeps,
        stage,
        ..card
    })
}

fn failed(pane: PaneId, batch: &Batch) -> Activity {
    let card = cards(pane, batch, batch.label());
    Activity {
        said: format!("Couldn’t write cards · {}", card.range),
        ..card
    }
}

fn cards(pane: PaneId, batch: &Batch, range: String) -> Activity {
    Activity {
        key: Key::Cards {
            pane,
            from: batch.from,
        },
        task: TaskId::StudyCards,
        said: String::new(),
        range,
        page: batch.from,
        keeps: false,
        stage: Stage::Failed,
    }
}
