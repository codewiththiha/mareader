//! One question the library asks the reader, and what answering it needs.

use mareader_library::gate::{Answered, Clash};
use mareader_library::screen::{Answer, FileAsk, Why};
use mareader_library::{Copied, Entry, Found, Library, MergeAsk, MoveAsk, Options, Placement};

use super::{Out, Pick, State, wall_ms};
use crate::toast::Toast;

/// One question, with what answering it needs.
#[derive(Clone, Debug, PartialEq)]
pub enum Question {
    /// A folder's name is taken at the top level, asked before the walk.
    Clash {
        clash: Clash,
        opts: Options,
    },
    File(FileAsk),
    Merge(MergeAsk),
    /// A moved book meets one of its name where it lands.
    Move(MoveAsk),
}

/// Questions alike enough to share "apply to all" and a count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Clash,
    Name,
    Held,
    Merge,
}

impl Question {
    pub fn kind(&self) -> Kind {
        match self {
            Self::Clash { .. } => Kind::Clash,
            Self::File(ask) if ask.why == Why::Name => Kind::Name,
            Self::Move(_) => Kind::Name,
            Self::File(_) => Kind::Held,
            Self::Merge(_) => Kind::Merge,
        }
    }

    pub fn offers(&self) -> &'static [Placement] {
        match self {
            Self::Clash { clash, .. } => clash.offers(),
            Self::File(ask) => ask.offers(),
            Self::Merge(_) => Placement::FOLDER_MERGE,
            Self::Move(ask) => ask.offers(),
        }
    }

    /// The arriving file, for an answer that copies it first.
    pub fn found(&self) -> Option<&Found> {
        match self {
            Self::Clash { .. } | Self::Move(_) => None,
            Self::File(ask) => Some(&ask.found),
            Self::Merge(ask) => Some(&ask.found),
        }
    }

    /// Whether `answer` lands a store copy that must be made first.
    pub fn copies(&self, answer: Placement, model: &Library) -> bool {
        match self {
            Self::Clash { .. } | Self::Move(_) => false,
            Self::File(_) => answer == Placement::KeepBoth,
            Self::Merge(ask) => {
                let copying = model.folder(ask.folder).is_some_and(|f| f.mode().copies());
                copying && matches!(answer, Placement::KeepBoth | Placement::Replace)
            }
        }
    }
}

impl State {
    /// The front question answered; `all` answers its kind behind it.
    pub fn answer(&mut self, answer: Placement, all: bool, now: f64, out: &mut Vec<Out>) {
        let Some(first) = self.asks.pop_front() else {
            return;
        };
        let kind = first.kind();
        let mut batch = vec![first];
        if all && matches!(kind, Kind::Held | Kind::Merge) {
            while self.asks.front().is_some_and(|q| q.kind() == kind) {
                batch.extend(self.asks.pop_front());
            }
        }
        for question in batch {
            if !question.offers().contains(&answer) {
                continue;
            }
            match question {
                Question::Clash { clash, opts } => {
                    self.answer_clash(&clash, opts, answer, now, out)
                }
                q if q.copies(answer, &self.model) => self.copy_for(q, answer, out),
                q => self.write_answer(q, answer, None, now, out),
            }
        }
    }

    fn answer_clash(
        &mut self,
        clash: &Clash,
        opts: Options,
        answer: Placement,
        now: f64,
        out: &mut Vec<Out>,
    ) {
        match self.model.answer_clash(clash, answer, wall_ms()) {
            Answered::Done(Some(shelf)) if answer == Placement::LinkOnly => {
                let name = self.model.shelf(shelf).map(|s| s.name.value.clone());
                let note = format!("Linked to “{}”.", name.unwrap_or_default());
                out.push(Out::Toast(Toast::info(note)));
            }
            Answered::Done(Some(shelf)) => self.reveal(Pick::Shelf(shelf), now),
            Answered::Done(None) => {}
            Answered::Walk { root, plan } => self.walk(root, opts, plan, now),
        }
    }

    /// Writes a file's or a merge's answer, its copy made if owed.
    pub fn write_answer(
        &mut self,
        question: Question,
        answer: Placement,
        copy: Option<&Copied>,
        now: f64,
        out: &mut Vec<Out>,
    ) {
        let wall = wall_ms();
        let answered = match &question {
            Question::Clash { .. } => return,
            Question::File(ask) => self.model.answer_file(ask, answer, copy, wall),
            Question::Merge(ask) => self.model.answer_merge(ask, answer, copy, wall),
            Question::Move(ask) => self.model.answer_move(ask, answer, wall),
        };
        match answered {
            Answer::Reveal(Entry::Book(id)) => self.reveal(Pick::Book(id), now),
            Answer::Reveal(Entry::Link(id)) => self.reveal(Pick::Link(id), now),
            Answer::Nothing if copy.is_none() && question.copies(answer, &self.model) => {
                let note = "The copy couldn't be made, so nothing changed.";
                out.push(Out::Toast(Toast::error(note)));
            }
            Answer::Landed(_) | Answer::Linked | Answer::Nothing => {}
        }
    }
}
