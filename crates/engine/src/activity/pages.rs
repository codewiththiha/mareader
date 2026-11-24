//! The page AI's work as the corner stack lists it: a page rewritten, a
//! pane's picked pages asked in groups, and its continuous run.

use mareader_library::AiTask;
use mareader_settings::{TaskId, language_name};
use mareader_workspace::PaneId;

use super::{Activity, Ending, Key, Stage};
use crate::page_ai::follow::{Follow, Standing, doing};
use crate::page_ai::groups::Batch;
use crate::page_ai::ledger::Ledger;
use crate::page_ai::state::{Gone, Handle, Run};

/// Turns a pane's own page into the page that shows it.
pub type PageOf<'a> = &'a dyn Fn(PaneId, usize) -> usize;

/// The page AI's jobs: each pane's continuous run and groups, then the
/// pages asked one by one.
pub fn of_pages(ledger: &Ledger, page_of: PageOf<'_>, now: f64) -> Vec<Activity> {
    let mut all = Vec::new();
    let mut follows: Vec<_> = ledger.follows.iter().collect();
    follows.sort_by_key(|(pane, _)| **pane);
    for (&pane, f) in follows {
        if let Some(standing) = ledger.standing(pane, now) {
            all.push(follow(pane, f, standing, page_of));
        }
    }
    let mut batches: Vec<_> = ledger.batches.iter().collect();
    batches.sort_by_key(|(pane, _)| **pane);
    all.extend(
        batches
            .into_iter()
            .filter_map(|(&p, b)| batch(p, b, page_of)),
    );
    let alone = |r: &&Run| r.failed.is_none() && owner(ledger, r.pane, r.id).is_none();
    all.extend(ledger.runs.iter().filter(alone).map(|r| page(r, page_of)));
    all
}

/// The job whose own visual run `run`'s bubble or result is. A
/// continuous run's page shows it while its bubble spins.
pub fn key_of(ledger: &Ledger, pane: PaneId, run: u64) -> Key {
    match owner(ledger, pane, run) {
        Some(Owner::Batch(batch)) => Key::Pages { pane, batch },
        Some(Owner::Follow(run)) => Key::Follow { pane, run },
        None => Key::Page { pane, run },
    }
}

/// How a page job gone from the list ended.
pub(super) fn ending(ledger: &Ledger, key: Key) -> Ending {
    let (id, quiet) = match key {
        Key::Page { run, .. } => (run, false),
        Key::Pages { batch, .. } => (batch, false),
        Key::Follow { run, .. } => (run, true),
        _ => return Ending::Quiet,
    };
    let failed = |r: &Run| r.id == id && r.failed.is_some();
    if matches!(key, Key::Page { .. }) && ledger.runs.iter().any(failed) {
        return Ending::Failed;
    }
    match ledger.gone(id) {
        Some(Gone::Stopped) => Ending::Stopped,
        Some(Gone::Failed) => Ending::Failed,
        // A continuous run's pages show their own results.
        None if quiet => Ending::Quiet,
        None => Ending::Landed,
    }
}

enum Owner {
    Batch(u64),
    /// The continuous run, by its own id.
    Follow(u64),
}

fn owner(ledger: &Ledger, pane: PaneId, run: u64) -> Option<Owner> {
    let batch = ledger.batches.get(&pane);
    if let Some(b) = batch.filter(|b| b.runs.iter().any(|(id, _)| *id == run)) {
        return Some(Owner::Batch(b.id));
    }
    let follow = ledger.follows.get(&pane);
    follow
        .filter(|f| f.runs.contains(&run))
        .map(|f| Owner::Follow(f.id))
}

fn page(run: &Run, page_of: PageOf<'_>) -> Activity {
    let pages = match run.last > run.page {
        true => format!("pages {}–{}", run.page + 1, run.last + 1),
        false => format!("page {}", run.page + 1),
    };
    let range = phrase(&run.task, &pages);
    let writing = run.handle.as_ref().is_some_and(Handle::writing);
    let (said, stage) = match writing {
        true => (capital(&range), Stage::Running(None)),
        false => (
            format!("{} · waiting for the model", capital(&range)),
            Stage::Queued,
        ),
    };
    Activity {
        key: Key::Page {
            pane: run.pane,
            run: run.id,
        },
        said,
        page: page_of(run.pane, run.page),
        stage,
        ..line(range)
    }
}

fn batch(pane: PaneId, b: &Batch, page_of: PageOf<'_>) -> Option<Activity> {
    let (from, to) = b.pages;
    let range = phrase(b.task()?, &format!("pages {}–{}", from + 1, to + 1));
    let total = b.total.max(1);
    let said = format!("{} · {} of {total} groups", capital(&range), b.finished);
    Some(Activity {
        key: Key::Pages { pane, batch: b.id },
        said,
        page: page_of(pane, from),
        stage: Stage::Running(Some(b.finished as f32 / total as f32)),
        ..line(range)
    })
}

fn follow(pane: PaneId, f: &Follow, standing: Standing, page_of: PageOf<'_>) -> Activity {
    let stage = match standing.paused {
        true => Stage::Queued,
        false => Stage::Running(standing.share),
    };
    Activity {
        key: Key::Follow { pane, run: f.id },
        said: standing.said,
        page: page_of(pane, f.seen.0),
        keeps: false,
        asks: standing.asks,
        stage,
        ..line(format!("{} as you read", doing(&f.task).1))
    }
}

/// A page job's line; its owner toasts a failure itself.
fn line(range: String) -> Activity {
    Activity {
        key: Key::Page {
            pane: PaneId(0),
            run: 0,
        },
        task: TaskId::SelectionAi,
        said: String::new(),
        range,
        page: 0,
        keeps: true,
        told: true,
        asks: false,
        stage: Stage::Running(None),
    }
}

/// "simplifying page 4", "translating pages 2–5 into Burmese".
fn phrase(task: &AiTask, pages: &str) -> String {
    match task {
        AiTask::Translate { language } => {
            let name = language_name(language).unwrap_or(language);
            format!("translating {pages} into {name}")
        }
        _ => format!("{} {pages}", doing(task).1),
    }
}

fn capital(words: &str) -> String {
    let mut chars = words.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}
