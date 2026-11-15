//! Hand moves on the page: the copy they may owe, then the landing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mareader_library::{BookId, Copied, Data, Found, LinkId, Move, Moved, Placement, ShelfId};

use super::Out;
use super::Question;
use super::books::counted;
use super::{CopyIn, Job, Pending, Pick, PickAction, Sheet, State, wall_ms};
use crate::command::Command;
use crate::toast::Toast;

pub use super::{CopyAnswer, CopyWork};

impl State {
    /// Lands a move, asking first if books must leave their folder.
    pub fn carry(&mut self, mv: Move, out: &mut Vec<Out>) {
        let owed = self.model.departure(&mv);
        match owed.is_empty() {
            true => self.land_moved(&mv, &[], out),
            false => self.ask_copy(CopyWork::Move { mv, owed }),
        }
    }

    /// Takes a shelf apart; a rung with books read in place asks first.
    pub fn apart(&mut self, id: ShelfId, out: &mut Vec<Out>) {
        match self.model.rung_books(id, false).is_empty() {
            true => self.take_apart(id, out),
            false => self.ask_copy(CopyWork::Apart(id)),
        }
    }

    /// The remove sheet's answer: the cascade counted in, copies asked.
    pub fn remove_entries(
        &mut self,
        (books, shelves): (&[BookId], &[ShelfId]),
        cascade: bool,
        forget: bool,
        out: &mut Vec<Out>,
    ) {
        self.sheet = None;
        let going = self.model.removing(books, shelves, cascade);
        if going.copies.is_empty() {
            self.remove(&going.books, &going.shelves, forget, out);
            return;
        }
        self.ask_copy(CopyWork::Removal {
            books: going.books,
            shelves: going.shelves,
            copies: going.copies,
            forget,
        });
    }

    fn ask_copy(&mut self, work: CopyWork) {
        self.sheet = Some(Sheet::Copy(Box::new(work)));
    }

    /// The copy ask answered: copies first, or the way without them.
    pub fn answer_copy(&mut self, answer: CopyAnswer, out: &mut Vec<Out>) {
        let Some(Sheet::Copy(work)) = self.sheet.take() else {
            return;
        };
        if answer == CopyAnswer::Without {
            self.without(*work, out);
            return;
        }
        let books = work.books(&self.model);
        let files: Vec<Found> = books.iter().filter_map(|b| self.found_of(*b)).collect();
        match files.is_empty() {
            true => self.departed(work, &HashMap::new(), out),
            false => self.copy_out(work, files, out),
        }
    }

    /// Copies books read in place, then finishes the gesture.
    pub fn copy_out(&mut self, work: Box<CopyWork>, files: Vec<Found>, out: &mut Vec<Out>) {
        let Some(store) = self.store() else {
            let note = "The library has no folder to keep books in.";
            out.push(Out::Toast(Toast::error(note)));
            return;
        };
        let tag = self.tag(Pending::Depart(work));
        let run = None;
        self.io.send(Job::Copy(Box::new(CopyIn {
            tag,
            run,
            store,
            files,
        })));
    }

    /// Copies the books a duplicate wants, then makes the duplicates.
    pub fn copy_to_duplicate(&mut self, picks: Vec<Pick>, files: Vec<Found>, out: &mut Vec<Out>) {
        let Some(store) = self.store() else {
            let note = "The library has no folder to keep books in.";
            out.push(Out::Toast(Toast::error(note)));
            return;
        };
        let tag = self.tag(Pending::Duplicate(picks));
        let run = None;
        self.io.send(Job::Copy(Box::new(CopyIn {
            tag,
            run,
            store,
            files,
        })));
    }

    /// Copies an answer's file first, then writes the answer.
    pub fn copy_for(&mut self, question: Question, answer: Placement, out: &mut Vec<Out>) {
        let (Some(store), Some(found)) = (self.store(), question.found().cloned()) else {
            let note = "The library has no folder to keep books in.";
            out.push(Out::Toast(Toast::error(note)));
            return;
        };
        let tag = self.tag(Pending::Keep(Box::new(question), answer));
        let files = vec![found];
        let (run, store) = (None, store);
        self.io.send(Job::Copy(Box::new(CopyIn {
            tag,
            run,
            store,
            files,
        })));
    }

    /// A book as the copier takes it: its file read in place.
    pub fn found_of(&self, id: BookId) -> Option<Found> {
        let book = self.model.book(id)?;
        let home = book.home()?;
        Some(Found {
            path: home.path.clone(),
            format: book.format,
            measure: mareader_library::Measure {
                content: book.content,
                stamp: home.stamp,
            },
            title: None,
            author: None,
        })
    }

    fn without(&mut self, work: CopyWork, out: &mut Vec<Out>) {
        match work {
            CopyWork::Move { mv, .. } => {
                let back = self.model.send_home(&mv, wall_ms());
                let note = match back.as_slice() {
                    [] => return,
                    [one] => format!("Put “{}” back in its place.", self.shelf_name(*one)),
                    many => format!("Put {} back in their places.", many.len()),
                };
                out.push(Out::Toast(Toast::info(note)));
            }
            CopyWork::Removal {
                books,
                shelves,
                forget,
                ..
            } => self.remove(&books, &shelves, forget, out),
            CopyWork::Apart(_) => {}
        }
    }

    /// The copies made: each book departs, then the gesture finishes.
    pub fn departed(
        &mut self,
        work: Box<CopyWork>,
        copies: &HashMap<PathBuf, Copied>,
        out: &mut Vec<Out>,
    ) {
        let wall = wall_ms();
        let books = work.books(&self.model);
        let mut done = Vec::new();
        for &id in &books {
            let path = self.model.book(id).and_then(|b| b.path());
            let copy = path.map(Path::to_path_buf).and_then(|p| copies.get(&p));
            if copy.is_some_and(|c| self.model.depart(id, c, wall)) {
                done.push(id);
            }
        }
        match *work {
            CopyWork::Move { mv, .. } => self.land_moved(&mv, &done, out),
            // A copy that failed leaves the rung standing.
            CopyWork::Apart(id) if done.len() == books.len() => self.take_apart(id, out),
            CopyWork::Apart(_) => {}
            CopyWork::Removal {
                books,
                shelves,
                forget,
                ..
            } => self.remove(&books, &shelves, forget, out),
        }
    }

    /// Lands what's clean; same-name books join the questions.
    fn land_moved(&mut self, mv: &Move, departed: &[BookId], out: &mut Vec<Out>) {
        let on = |b: &BookId, s: Option<ShelfId>| {
            s.is_some_and(|s| self.model.shelves_of(*b).contains(&s))
        };
        let already = mv.books.iter().filter(|b| on(b, mv.to)).count();
        let leaving = (mv.books.iter())
            .filter(|b| !mv.keep && mv.from != mv.to && on(b, mv.from))
            .count();
        let moved = self.model.land_move(mv, departed, wall_ms());
        if mv.books.len() + mv.shelves.len() > 1 {
            self.picks.apply(PickAction::Clear);
        }
        let note = self.moved_note(mv, &moved, (already, leaving));
        self.asks.extend(moved.asks.into_iter().map(Question::Move));
        out.extend(note.map(|n| Out::Toast(Toast::info(n))));
    }

    fn moved_note(
        &self,
        mv: &Move,
        moved: &Moved,
        (already, leaving): (usize, usize),
    ) -> Option<String> {
        if let Some(&shelf) = moved.stayed.first() {
            let name = self.shelf_name(shelf);
            return Some(format!("“{name}” stayed: its books' copies weren't made."));
        }
        let title = |id: &BookId| self.model.book(*id).map(|b| b.title.value.clone());
        let link = |id: &LinkId| self.model.link(*id).map(|l| l.name.value.clone());
        let counts = (moved.books, moved.shelves, moved.links);
        let what = match (counts, &mv.books[..], &mv.shelves[..], &mv.links[..]) {
            ((0, 0, 0), ..) => return None,
            ((1, 0, 0), [one], ..) => format!("“{}”", title(one).unwrap_or_default()),
            ((0, 1, 0), _, [one], _) => format!("“{}”", self.shelf_name(*one)),
            ((0, 0, 1), .., [one]) => format!("“{}”", link(one).unwrap_or_default()),
            ((b, s, l), ..) => listed(&[
                (b, "book", "books"),
                (s, "shelf", "shelves"),
                (l, "link", "links"),
            ]),
        };
        let there = match mv.to {
            Some(to) => format!("“{}”", self.shelf_name(to)),
            None => "the top of the library".into(),
        };
        let note = match (mv.to, mv.from) {
            (_, _) if moved.shelves + moved.links == 0 && already == moved.books => {
                match moved.books {
                    1 => format!("{what} is already on {there}."),
                    _ => format!("They're already on {there}."),
                }
            }
            (None, Some(from)) if moved.shelves == 0 => {
                format!("Took {what} off “{}”.", self.shelf_name(from))
            }
            // Shelves and links never stay behind: they always move.
            _ if leaving + moved.shelves + moved.links > 0 => format!("Moved {what} to {there}."),
            _ => format!("Added {what} to {there}."),
        };
        Some(note)
    }

    /// Takes a shelf apart now: its shelves move up, a rung's books too.
    pub fn take_apart(&mut self, id: ShelfId, out: &mut Vec<Out>) {
        let name = self.shelf_name(id);
        let home = self.model.seat(id).flatten();
        let Some(done) = self.model.remove_shelf(id, wall_ms()) else {
            return;
        };
        self.settle();
        let note = match (done.books, home) {
            (0, _) => format!("Took “{name}” apart."),
            (_, Some(home)) => format!(
                "Took “{name}” apart. Its books are on “{}” now.",
                self.shelf_name(home)
            ),
            (_, None) => format!("Took “{name}” apart. Its books stay in the library."),
        };
        out.push(Out::Toast(Toast::info(note)));
    }

    /// Removes books, then shelves deepest first, and says so.
    pub fn remove(
        &mut self,
        books: &[BookId],
        shelves: &[ShelfId],
        forget: bool,
        out: &mut Vec<Out>,
    ) {
        let now = wall_ms();
        let first = books.first().and_then(|id| self.model.book(*id));
        let first = first.map(|b| b.title.value.clone()).unwrap_or_default();
        let shelf = shelves
            .first()
            .map(|s| self.shelf_name(*s))
            .unwrap_or_default();
        let receipt = self.model.receipt(books);
        let reset_books: Vec<BookId> = if forget {
            books
                .iter()
                .copied()
                .filter(|id| self.model.book(*id).is_some())
                .collect()
        } else {
            Vec::new()
        };
        let mut reset_paths = Vec::new();
        for id in &reset_books {
            if let Some(book) = self.model.book(*id) {
                for location in &book.locations {
                    reset_paths.push(location.path.clone());
                    if let Some(from) = &location.from {
                        reset_paths.push(from.clone());
                    }
                }
            }
        }
        reset_paths.sort();
        reset_paths.dedup();
        let data = match forget {
            true => Data::Delete,
            false => Data::Keep,
        };
        let done = self.model.remove_books(books, data, now);
        if forget && done > 0 {
            out.push(Out::Command(Command::ForgetPositions {
                books: reset_books,
                paths: reset_paths,
            }));
        }
        let gone = (shelves.iter())
            .filter(|id| self.model.remove_shelf(**id, now).is_some())
            .count();
        self.picks.apply(PickAction::Clear);
        self.settle();
        let said = match (done, gone) {
            (0, 0) => return,
            (1, 0) => format!("Removed “{first}”."),
            (n, 0) => format!("Removed {n} books."),
            (0, 1) => format!("Removed “{shelf}”."),
            (0, s) => format!("Removed {s} shelves."),
            (n, s) => format!(
                "Removed {} and {}.",
                counted(n, "book", "books"),
                counted(s, "shelf", "shelves")
            ),
        };
        let note = match receipt.hidden {
            0 => said,
            _ => format!("{said} The folder's Add menu can give them back."),
        };
        out.push(Out::Toast(Toast::info(note)));
    }
}

/// "2 books, 1 shelf and 3 links": the counts that aren't nought.
pub fn listed(counts: &[(usize, &str, &str)]) -> String {
    let parts: Vec<String> = (counts.iter())
        .filter(|(n, ..)| *n > 0)
        .map(|&(n, one, many)| counted(n, one, many))
        .collect();
    match parts.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}
