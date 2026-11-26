//! Skills asked of a selection or a page: their runs and their answers.
//!
//! An ask keeps where it came from, so its card points there and rides the
//! page. Selection answers remain in the session when their card is closed.
//! See `docs/selection-ai.md`.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use mareader_library::{AiSelectionId, BookId, NewAiSelection, Spot};
use mareader_session::Document;
use mareader_settings::Skill;
use mareader_study::Failure;
use mareader_study::pending::News;
use mareader_study::skill::{Scripted, SkillAsk, SkillJob, Skiller};
use mareader_workspace::PaneId;

pub(crate) mod restore;

/// Page-menu asks kept at once; selections are retained for the session.
pub const MOST: usize = 8;

/// An ask's handle, unique for the session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AskId(pub u64);

/// What an ask's card points at.
#[derive(Clone, Debug, PartialEq)]
pub enum Anchor {
    /// Characters on pages, as the selection held them.
    Text(Vec<(usize, Range<usize>)>),
    /// A spot on a page, in page points: where the page menu opened. `near`
    /// is the character closest to it, which a reveal brings into view.
    Spot {
        page: usize,
        x: f32,
        y: f32,
        near: usize,
    },
}

impl Anchor {
    /// The page it starts on.
    pub fn page(&self) -> usize {
        self.reveal().0
    }

    /// The page and characters a reveal brings into view.
    pub fn reveal(&self) -> (usize, Range<usize>) {
        match self {
            Self::Text(spans) => spans.first().cloned().unwrap_or((0, 0..0)),
            Self::Spot { page, near, .. } => (*page, *near..near.saturating_add(1)),
        }
    }
}

/// Where an ask's answer is.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Waiting,
    /// What has come so far.
    Partial(String),
    Ready(String),
    Failed(Failure),
    /// The app closed before the answer arrived.
    Interrupted,
}

impl Answer {
    /// Done, well or not.
    pub fn landed(&self) -> bool {
        matches!(self, Self::Ready(_) | Self::Failed(_) | Self::Interrupted)
    }

    /// The text so far; blank while none has come.
    pub fn text(&self) -> &str {
        match self {
            Self::Partial(text) | Self::Ready(text) => text,
            Self::Waiting | Self::Failed(_) | Self::Interrupted => "",
        }
    }
}

/// One skill asked of some text.
pub struct Ask {
    pub id: AskId,
    pub pane: PaneId,
    /// The document it was asked in, by number.
    pub doc: u64,
    pub skill: Skill,
    /// What the card calls it, as when asked.
    pub name: String,
    pub anchor: Anchor,
    /// The library book and durable selection spot, if available.
    pub book: Option<BookId>,
    pub spot: Option<Spot>,
    /// Whether the answer card is open.
    pub open: bool,
    /// The saved record, if pinning was requested.
    pub pinned: Option<AiSelectionId>,
    pub pin_status: crate::library::SaveStatus,
    pub(crate) auto_pin: bool,
    pub answer: Answer,
    /// Who answers, as the card's small print names them.
    pub by: String,
    text: String,
    language: Option<String>,
    path: Option<PathBuf>,
    layout: Option<Weak<Document>>,
    job: Option<Box<SkillJob>>,
}

impl Ask {
    /// A snapshot for the library, preserving a waiting request honestly.
    pub(crate) fn record(&self) -> Option<NewAiSelection> {
        let spot = self.spot?;
        let result = match &self.answer {
            Answer::Ready(text) => text.clone(),
            _ => String::new(),
        };
        Some(NewAiSelection {
            skill: self.skill.key(),
            name: self.name.clone(),
            spot,
            source: self.text.clone(),
            language: self.language.clone(),
            result,
            pending: !matches!(self.answer, Answer::Ready(_)),
            by: self.by.clone(),
        })
    }

    /// The selected text originally sent to the skill.
    pub fn source(&self) -> &str {
        &self.text
    }

    /// The translation target fixed when this ask began, if any.
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    /// How long the asked text is, in characters.
    pub fn asked_chars(&self) -> usize {
        self.text.chars().count()
    }
}

/// What a skill card or menu asks of the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Runs a skill on the pane's selection.
    Run { pane: PaneId, skill: Skill },
    /// Translates the selection into a request-specific language.
    Translate { pane: PaneId, tag: String },
    /// Runs a skill on a page; its card points at a spot on it.
    RunPage {
        pane: PaneId,
        page: usize,
        x: f32,
        y: f32,
        skill: Skill,
    },
    /// Asks the same again.
    Retry(AskId),
    /// Hides a selection card; page-menu cards still close their run.
    Close(AskId),
    /// Cancels a running request.
    Cancel(AskId),
    /// Removes a selection answer.
    Remove(AskId),
    /// Toggles whether a selection answer is saved.
    Pin(AskId),
    /// Goes back to a card's place, and shows it.
    Show(AskId),
}

/// Every ask, and who answers them.
pub struct State {
    asks: Vec<Ask>,
    next: u64,
    runner: Arc<dyn Skiller>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            asks: Vec::new(),
            next: 1,
            runner: Arc::new(Scripted::Nobody),
        }
    }
}

/// A new ask, before it has an id.
pub(crate) struct Asked {
    pub pane: PaneId,
    pub doc: u64,
    pub skill: Skill,
    pub name: String,
    pub anchor: Anchor,
    pub book: Option<BookId>,
    pub spot: Option<Spot>,
    pub text: String,
    pub language: Option<String>,
    pub path: Option<PathBuf>,
    pub layout: Option<Weak<Document>>,
    pub auto_pin: bool,
}

impl State {
    pub fn asks(&self) -> &[Ask] {
        &self.asks
    }

    pub fn get(&self, id: AskId) -> Option<&Ask> {
        self.asks.iter().find(|a| a.id == id)
    }

    pub(crate) fn pin_statuses(
        &mut self,
        status: impl Fn(AiSelectionId) -> crate::library::SaveStatus,
    ) {
        for ask in &mut self.asks {
            if let Some(id) = ask.pinned {
                ask.pin_status = status(id);
            }
        }
    }

    pub(crate) fn get_mut(&mut self, id: AskId) -> Option<&mut Ask> {
        self.asks.iter_mut().find(|a| a.id == id)
    }

    /// Who answers from now on; runs under way keep theirs.
    pub(crate) fn set_runner(&mut self, runner: Arc<dyn Skiller>) {
        self.runner = runner;
    }

    /// Starts `asked`; excess page-menu asks go oldest first.
    pub(crate) fn start(&mut self, asked: Asked) -> AskId {
        let id = AskId(self.next);
        self.next += 1;
        let key = asked.skill.key();
        let job = self.runner.run(SkillAsk {
            skill: key.clone(),
            text: asked.text.clone(),
            language: asked.language.clone(),
        });
        self.asks.push(Ask {
            id,
            pane: asked.pane,
            doc: asked.doc,
            skill: asked.skill,
            name: asked.name,
            anchor: asked.anchor,
            book: asked.book,
            spot: asked.spot,
            open: true,
            pinned: None,
            pin_status: crate::library::SaveStatus::Saved,
            auto_pin: asked.auto_pin,
            answer: Answer::Waiting,
            by: self.runner.by(&key),
            text: asked.text,
            language: asked.language,
            path: asked.path,
            layout: asked.layout,
            job: Some(job),
        });
        while self
            .asks
            .iter()
            .filter(|a| matches!(a.anchor, Anchor::Spot { .. }))
            .count()
            > MOST
        {
            if let Some(i) = self
                .asks
                .iter()
                .position(|a| matches!(a.anchor, Anchor::Spot { .. }))
            {
                self.asks.remove(i);
            }
        }
        id
    }

    /// Asks `id` again, from the start.
    pub(crate) fn retry(&mut self, id: AskId) -> bool {
        let runner = Arc::clone(&self.runner);
        let Some(ask) = self.asks.iter_mut().find(|a| a.id == id) else {
            return false;
        };
        let key = ask.skill.key();
        ask.job = Some(runner.run(SkillAsk {
            skill: key.clone(),
            text: ask.text.clone(),
            language: ask.language.clone(),
        }));
        ask.by = runner.by(&key);
        ask.answer = Answer::Waiting;
        true
    }

    /// Hides a selection card, leaving its run and answer intact.
    pub(crate) fn close(&mut self, id: AskId) -> bool {
        let Some(ask) = self.asks.iter_mut().find(|a| a.id == id) else {
            return false;
        };
        if matches!(ask.anchor, Anchor::Spot { .. }) {
            return self.remove(id);
        }
        std::mem::replace(&mut ask.open, false)
    }

    pub(crate) fn show(&mut self, id: AskId) -> bool {
        let Some(ask) = self.asks.iter_mut().find(|a| a.id == id) else {
            return false;
        };
        ask.open = true;
        true
    }

    /// Stops a retry and returns to the last saved answer.
    pub(crate) fn cancel_to(&mut self, id: AskId, text: String, by: String) -> bool {
        let Some(ask) = self.get_mut(id) else {
            return false;
        };
        ask.job = None;
        ask.answer = Answer::Ready(text);
        ask.by = by;
        true
    }

    /// Drops an ask and its job.
    pub(crate) fn remove(&mut self, id: AskId) -> bool {
        let before = self.asks.len();
        self.asks.retain(|a| a.id != id);
        self.asks.len() != before
    }

    /// Drops the asks of `pane` not about `doc`; none drops them all.
    pub(crate) fn keep_pane(&mut self, pane: PaneId, doc: Option<u64>) {
        self.asks.retain(|a| {
            matches!(a.anchor, Anchor::Text(_)) || a.pane != pane || Some(a.doc) == doc
        });
    }

    /// Keeps the asks whose pane still reads their document; true if any went.
    pub(crate) fn keep_read(&mut self, reads: impl Fn(PaneId) -> Option<u64>) -> bool {
        let before = self.asks.len();
        self.asks
            .retain(|a| matches!(a.anchor, Anchor::Text(_)) || reads(a.pane) == Some(a.doc));
        self.asks.len() != before
    }

    /// Takes in news; the asks that landed, and whether any changed.
    pub(crate) fn receive(&mut self) -> (Vec<AskId>, bool) {
        let (mut landed, mut changed) = (Vec::new(), false);
        for ask in &mut self.asks {
            let Some(job) = ask.job.as_mut() else {
                continue;
            };
            for news in job.news() {
                changed = true;
                ask.answer = match news {
                    News::Partial(text) => Answer::Partial(text),
                    News::Done(Ok(text)) => Answer::Ready(text),
                    News::Done(Err(failure)) => Answer::Failed(failure),
                };
            }
            if ask.answer.landed() {
                ask.job = None;
                landed.push(ask.id);
            }
        }
        (landed, changed)
    }

    /// Whether a run is still under way.
    pub fn running(&self) -> bool {
        self.asks.iter().any(|a| a.job.is_some())
    }
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;
