//! The AI's work going on, as the corner stack lists it.
//!
//! Each owner keeps its own jobs; [`of_study`] and its kin read what each is
//! doing into one list a frame, so the window says it all in one place. A
//! job's [`Key`] stays the same from its start to its end, and the
//! [`Ledger`] keeps how each ended once its owner forgets it. See
//! `docs/study-mode.md` § The corner stack.

mod ledger;

pub mod place;

pub use ledger::{End, LINGER, Ledger, News, Row};

use mareader_settings::TaskId;
use mareader_study::mode::{Batch, Via};
use mareader_workspace::PaneId;

use crate::library::{self, RunEnd, RunId};
use crate::skills::{self, AskId};
use crate::study::state::{self as words, Key as Ask};
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
    /// The library's AI organizer, by its run.
    Organize { run: RunId },
    /// The AI Dictionary explaining a word `pane` asked.
    Explain { pane: PaneId, ask: Ask },
    /// A skill's card, asked of a selection or a page.
    Skill { pane: PaneId, ask: AskId },
}

impl Key {
    pub fn scope(self) -> Scope {
        match self {
            Self::Cards { pane, .. } | Self::Explain { pane, .. } | Self::Skill { pane, .. } => {
                Scope::Pane(pane)
            }
            Self::Organize { .. } => Scope::Global,
        }
    }

    /// The word on its stop button: organizing pauses, keeping its work.
    pub fn stop_word(self) -> &'static str {
        match self {
            Self::Cards { .. } | Self::Explain { .. } | Self::Skill { .. } => "Cancel",
            Self::Organize { .. } => "Pause",
        }
    }
}

/// How a job that left the list ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// Its result is in.
    Landed,
    /// The reader stopped it.
    Stopped,
    Failed,
    /// It ended with nothing to show, and its owner said so.
    Quiet,
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
    /// Whether its owner toasts its failure itself; the stack then doesn't.
    pub told: bool,
    pub stage: Stage,
}

impl Activity {
    /// The words once it landed: "Cards ready · pages 4–6".
    pub fn done_said(&self) -> String {
        match (self.key, self.keeps) {
            (Key::Cards { .. }, true) => format!("Cards ready · {}", self.range),
            (Key::Cards { .. }, false) => "Next cards ready".to_owned(),
            (Key::Organize { .. }, _) => format!("Shelf plan ready · {}", self.range),
            (Key::Explain { .. }, _) => format!("Explained {}", self.range),
            (Key::Skill { .. }, _) => format!("{} ready", self.range),
        }
    }

    /// The words once it failed, on its line and in its toast.
    pub fn failed_said(&self) -> String {
        match self.key {
            Key::Cards { .. } => format!("Couldn’t write the cards for {}", self.range),
            Key::Organize { .. } => format!("Couldn’t organize {}", self.range),
            Key::Explain { .. } => format!("Couldn’t explain {}", self.range),
            Key::Skill { .. } => format!("{} didn’t finish", self.range),
        }
    }

    /// The toast once it was stopped.
    pub fn stopped_said(&self) -> String {
        match self.key {
            Key::Cards { .. } => format!("Stopped writing the cards for {}", self.range),
            Key::Organize { .. } => "Organizing paused. The library can resume it.".to_owned(),
            Key::Explain { .. } => format!("Stopped explaining {}", self.range),
            Key::Skill { .. } => format!("Stopped {}", self.range),
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

/// The library's AI organizer, while it plans or files.
pub fn of_library(library: &library::State) -> Vec<Activity> {
    let going = library.runs.list().iter().filter(|run| run.stoppable());
    let activity = |run: &library::Run| Activity {
        key: Key::Organize { run: run.id },
        task: TaskId::Organize,
        said: format!("{} · {}", run.headline().trim_end_matches('…'), run.label),
        range: run.label.clone(),
        page: 0,
        keeps: true,
        told: true,
        stage: Stage::Running(run.fraction()),
    };
    going.map(activity).collect()
}

/// The AI Dictionary explaining `word` for `pane`; `page` is its mark's,
/// which View goes back to. A word kept by nothing has none to go to.
pub fn explain(pane: PaneId, ask: Ask, word: &str, page: Option<usize>) -> Activity {
    let range = format!("“{word}”");
    Activity {
        key: Key::Explain { pane, ask },
        task: TaskId::Explain,
        said: format!("Explaining {range}"),
        range,
        page: page.unwrap_or(0),
        keeps: page.is_some(),
        told: false,
        stage: Stage::Running(None),
    }
}

/// The skills' cards still being answered.
pub fn of_skills(skills: &skills::State) -> Vec<Activity> {
    let going = skills.asks().iter().filter(|ask| !ask.answer.landed());
    going.map(skill).collect()
}

/// A skill's card as a line; once landed, its owner tells how it went.
pub fn skill(ask: &skills::Ask) -> Activity {
    let page = ask.anchor.page();
    Activity {
        key: Key::Skill {
            pane: ask.pane,
            ask: ask.id,
        },
        task: TaskId::SelectionAi,
        said: format!("{} · page {}", ask.name, page + 1),
        range: ask.name.clone(),
        page,
        keeps: true,
        told: false,
        stage: Stage::Running(None),
    }
}

/// The owners `ending` asks how a job ended.
pub struct Owners<'a> {
    pub study: &'a State,
    pub library: &'a library::State,
    pub words: &'a words::State,
    pub skills: &'a skills::State,
}

/// How the job `key` names, gone from the list, ended.
pub fn ending(owners: Owners<'_>, key: Key) -> Ending {
    let Owners {
        study,
        library,
        words,
        skills,
    } = owners;
    match key {
        // A deck's job ends in its cards, unless study mode went off.
        Key::Cards { pane, .. } if study.on(pane) => Ending::Landed,
        Key::Cards { .. } => Ending::Stopped,
        Key::Organize { run } => match library.runs.ended(run) {
            Some(RunEnd::Planned) => Ending::Landed,
            Some(RunEnd::Empty) => Ending::Quiet,
            Some(RunEnd::Failed) => Ending::Failed,
            None => Ending::Stopped,
        },
        Key::Explain { ask, .. } if words.answer(ask).is_some() => Ending::Landed,
        Key::Explain { ask, .. } if words.failure(ask).is_some() => Ending::Failed,
        Key::Explain { .. } => Ending::Stopped,
        // A landed card tells by `Event::SkillLanded`; a closed one stopped.
        Key::Skill { ask, .. } if skills.get(ask).is_some() => Ending::Quiet,
        Key::Skill { .. } => Ending::Stopped,
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
        told: false,
        stage: Stage::Failed,
    }
}
