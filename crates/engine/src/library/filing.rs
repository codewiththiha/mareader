//! Driving a filing run: what the library asks, hears and files.

use std::collections::BTreeSet;

use mareader_library::picks::{Pick, PickAction};
use mareader_library::{Book, BookId, ShelfId};

use super::{
    Action, Applied, Ask, Beat, Heard, Loose, Organizer, Out, Paused, Phase, Proposal, Sheet, Spot,
    Standing, State, apply, counted, natural, organizable, undo, wall_ms,
};
use crate::command::Command;
use crate::toast::Toast;

impl State {
    pub fn organizer(&self) -> &Organizer {
        &self.organizer
    }

    /// What to ask about `picked`, else every loose book; or why not.
    pub fn ask_organize(&self, picked: Option<&[BookId]>) -> Result<Ask, &'static str> {
        if !self.held.is_empty() {
            return Err("Already organizing. Wait until it's done.");
        }
        let model = &self.model;
        let mut books: Vec<&Book> = match picked {
            Some(ids) => ids.iter().filter_map(|id| model.book(*id)).collect(),
            None => model.books().collect(),
        };
        books.retain(|b| organizable(model, b));
        if books.is_empty() {
            return Err(match picked {
                Some(_) => "None of these can be organized: only imported books on no shelf can.",
                None => "Nothing to organize: every imported book is on a shelf.",
            });
        }
        books.sort_by(|a, b| natural(&a.title.value, &b.title.value));
        let books = (books.into_iter())
            .map(|b| Loose {
                id: b.id,
                title: b.title.value.clone(),
                author: b.author.value.clone(),
            })
            .collect();
        let shelves = (model.shelves())
            .map(|s| Standing {
                id: s.id,
                name: s.name.value.clone(),
                parent: model.parent_of(s),
            })
            .collect();
        Ok(Ask {
            books,
            shelves,
            whole: picked.is_none(),
        })
    }

    /// What resuming asks: its books still loose, and newcomers if whole.
    pub fn ask_resume(&self) -> Result<Ask, &'static str> {
        let Some(paused) = &self.organizer.paused else {
            return self.ask_organize(None);
        };
        let gone = "Nothing left to organize: the paused run's books are all \
                    on shelves or removed.";
        let mut ask = match self.ask_organize(None) {
            Err(why) if !self.held.is_empty() => return Err(why),
            Err(_) => return Err(gone),
            Ok(ask) => ask,
        };
        if !paused.whole {
            ask.books.retain(|b| paused.books.contains(&b.id));
            ask.whole = false;
        }
        match ask.books.is_empty() {
            true => Err(gone),
            false => Ok(ask),
        }
    }

    /// The paused run as the service keeps it; a card shows it when idle.
    pub fn set_paused(&mut self, paused: Option<Paused>) {
        self.organizer.paused = paused;
        self.paused_card();
    }

    /// A paused run's card shows while nothing else runs or waits.
    fn paused_card(&mut self) {
        let ended = self.organizer.paused_card.take();
        self.runs.finish(ended);
        let Some(paused) = self.organizer.paused.as_ref() else {
            return;
        };
        if !self.held.is_empty() {
            return;
        }
        let phase = match paused.ready {
            true => Phase::Ready,
            false => Phase::Paused,
        };
        let label = match paused.ready {
            true => format!("{} to review", counted(paused.books.len(), "book", "books")),
            false => paused.said(),
        };
        let (done, total) = (paused.done, paused.total);
        let run = self.runs.start(label, phase);
        let name = String::new();
        self.runs.beat(Beat {
            run,
            phase,
            done,
            total,
            name,
        });
        self.runs.show_now();
        self.organizer.paused_card = Some(run);
    }

    /// Asks whether a new run over `picked` replaces the paused one.
    pub fn ask_replace(&mut self, picked: Option<Vec<BookId>>) {
        self.sheet = Some(Sheet::Replace(picked));
    }

    /// A run began for `books`; its card says what with `label`.
    pub fn organizing(&mut self, books: impl IntoIterator<Item = BookId>, label: &str) {
        self.organizer_end();
        self.held = books.into_iter().collect();
        self.paused_card();
        let label = format!("{} books · {label}", self.held.len());
        self.organizer.run = Some(self.runs.start(label, Phase::Shelving));
        // Held books can't move, so none stay picked.
        let held = &self.held;
        let keep: Vec<Pick> = (self.picks.items().into_iter())
            .filter(|p| !matches!(p, Pick::Book(b) if held.contains(b)))
            .collect();
        let action = match keep.is_empty() {
            true => PickAction::Clear,
            false => PickAction::Only(keep),
        };
        self.picks.apply(action);
    }

    pub fn heard(&mut self, heard: Heard, out: &mut Vec<Out>) {
        match heard {
            Heard::Progress {
                filing,
                done,
                total,
            } => {
                let Some(run) = self.organizer.run else {
                    return;
                };
                let phase = if filing {
                    Phase::Filing
                } else {
                    Phase::Shelving
                };
                let name = String::new();
                self.runs.beat(Beat {
                    run,
                    phase,
                    done,
                    total,
                    name,
                });
            }
            Heard::Plan(plan) => {
                let ended = self.organizer.run.take();
                self.runs.finish(ended);
                if plan.placed.is_empty() {
                    self.organizer_end();
                    let said = "The AI found no shelf that suits these books.";
                    out.push(Out::Toast(Toast::info(said)));
                } else {
                    self.organizer.review = Some(Box::new(plan));
                    // The plan shows on the library page; elsewhere, a nudge.
                    if !self.showing {
                        let said = "The AI's shelf plan is ready. Open the library to review it.";
                        out.push(Out::Toast(Toast::info(said)));
                    }
                }
            }
            Heard::Failed(toast) => {
                self.organizer_end();
                // What was done is kept; the toast offers to carry on.
                let resume = Command::Library(Action::ResumeOrganizing);
                let toast = match &self.organizer.paused {
                    Some(p) if !p.ready => toast.action("Resume", resume),
                    _ => toast,
                };
                out.push(Out::Toast(toast));
            }
        }
    }

    /// The run stopped, or its plan was applied or let go: books move again.
    pub fn organizer_end(&mut self) {
        let ended = self.organizer.run.take();
        self.runs.finish(ended);
        let paused = self.organizer.paused.take();
        let ended_card = self.organizer.paused_card.take();
        self.runs.finish(ended_card);
        self.organizer = Organizer::default();
        // A run that ends frees its books, or they never move again.
        self.held.clear();
        self.set_paused(paused);
    }

    pub fn apply_organized(&mut self, out: &mut Vec<Out>) {
        let Some(plan) = self.organizer.review.take() else {
            return;
        };
        self.organizer_end();
        let done = apply(&mut self.model, &plan, wall_ms());
        let books = done.filed.len();
        let shelves: BTreeSet<ShelfId> = done.filed.iter().map(|(_, s)| *s).collect();
        let mut said = format!(
            "Filed {} on {}",
            counted(books, "book", "books"),
            counted(shelves.len(), "shelf", "shelves")
        );
        if !done.made.is_empty() {
            said.push_str(&format!(", {} new", done.made.len()));
        }
        said.push('.');
        if done.skipped > 0 {
            let changed = counted(done.skipped, "book", "books");
            said.push_str(&format!(" {changed} changed meanwhile and stayed put."));
        }
        let toast = match books {
            0 => Toast::info(said),
            _ => Toast::info(said).undo(Command::Library(Action::UndoOrganized(Box::new(done)))),
        };
        out.push(Out::Toast(toast));
    }

    /// A shelf move without the books a run holds; a toast says why.
    pub fn undo_organized(&mut self, done: &Applied) {
        undo(&mut self.model, done, wall_ms());
    }

    /// A made-up run over the loose books, stopped at 3 of 8 or `ready`.
    pub fn organize_paused_demo(&mut self, ready: bool) {
        let Ok(ask) = self.ask_organize(None) else {
            return;
        };
        let books = ask.books.iter().map(|b| b.id).collect();
        let done = if ready { 8 } else { 3 };
        self.set_paused(Some(Paused {
            done,
            total: 8,
            books,
            whole: true,
            ready,
        }));
    }

    /// A made-up run over the loose books, its plan up if `review`.
    pub fn organize_demo(&mut self, review: bool) {
        let Ok(ask) = self.ask_organize(None) else {
            return;
        };
        let books: Vec<BookId> = ask.books.iter().map(|b| b.id).collect();
        self.organizing(books.iter().copied(), "Fast model");
        let mut out = Vec::new();
        let progress = Heard::Progress {
            filing: true,
            done: 3,
            total: 4,
        };
        self.heard(progress, &mut out);
        self.runs.show_now();
        if !review {
            return;
        }
        let shelves = vec![
            ("Fiction".to_owned(), None),
            ("Classics".to_owned(), Some(Spot::New(0))),
            ("Essays".to_owned(), None),
        ];
        let (left, filed) = books
            .split_last()
            .map_or((None, &[][..]), |(l, f)| (Some(*l), f));
        let spots = [Spot::New(1), Spot::New(0), Spot::New(2)];
        let placed = (filed.iter().zip(spots.iter().cycle()))
            .map(|(b, s)| (*b, *s))
            .collect();
        let plan = Proposal {
            shelves,
            placed,
            left: left.into_iter().collect(),
        };
        self.heard(Heard::Plan(plan), &mut out);
    }
}
