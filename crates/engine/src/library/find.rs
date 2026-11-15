//! Finding a lost book again: its file picked, or a folder searched.

use std::path::Path;

use mareader_library::disk::{FoundAgain, Sought};
use mareader_library::{BookId, Format, Measure, RelinkError};

use super::{Action, Job, Out, Relink, Sheet, State};
use crate::command::Command;

pub use super::{Finding, Step};

impl State {
    /// The find-again sheet's work.
    pub fn find_act(&mut self, action: Action, out: &mut Vec<Out>) {
        match action {
            Action::FindAgain(book) => {
                let step = Step::Ask;
                self.sheet = Some(Sheet::Find(Finding { book, step }));
            }
            Action::Relink { book, path } => {
                let path = std::fs::canonicalize(&path).unwrap_or(path);
                self.finding(book, Step::Working("Checking the file…".into()));
                let force = false;
                self.io.send(Job::Relink(Relink { book, path, force }));
            }
            Action::UseAnyway(book) => {
                if let Some(Step::Differs {
                    path,
                    format,
                    measure,
                }) = self.step_of(book).cloned()
                {
                    let done = self.model.relink(book, &path, format, measure, true);
                    self.relinked(book, done, out);
                }
            }
            Action::Seek { book, folder } => {
                let Some(sought) = self.sought(book) else {
                    return;
                };
                let name = folder_name(&folder);
                self.finding(book, Step::Working(format!("Looking in “{name}”…")));
                self.io.send(Job::Seek {
                    book,
                    folder,
                    sought,
                });
            }
            _ => {}
        }
    }

    fn step_of(&self, book: BookId) -> Option<&Step> {
        match &self.sheet {
            Some(Sheet::Find(f)) if f.book == book => Some(&f.step),
            _ => None,
        }
    }

    /// Moves the find-again sheet on, if it's still for `book`.
    fn finding(&mut self, book: BookId, step: Step) {
        if let Some(Sheet::Find(f)) = &mut self.sheet
            && f.book == book
        {
            f.step = step;
        }
    }

    /// What a lost book's file is looked for by.
    pub fn sought(&self, book: BookId) -> Option<Sought> {
        let book = self.model.book(book)?;
        let last = book.locations.first();
        Some(Sought {
            content: book.content,
            format: book.format,
            name: last.and_then(|l| l.path.file_name()).map(ToOwned::to_owned),
            size: last.map(|l| l.stamp.size).filter(|&s| s > 0),
        })
    }

    pub fn relink_measured(
        &mut self,
        relink: Relink,
        measured: Option<(Format, Measure)>,
        out: &mut Vec<Out>,
    ) {
        let book = relink.book;
        let Some((format, measure)) = measured else {
            self.finding(book, Step::Note("That file couldn't be read.".into()));
            return;
        };
        let done = self
            .model
            .relink(book, &relink.path, format, measure, relink.force);
        match done {
            Err(RelinkError::Differs) => {
                let path = relink.path;
                self.finding(
                    book,
                    Step::Differs {
                        path,
                        format,
                        measure,
                    },
                );
            }
            done => self.relinked(book, done, out),
        }
    }

    pub fn sought_found(
        &mut self,
        book: BookId,
        folder: &Path,
        found: FoundAgain,
        out: &mut Vec<Out>,
    ) {
        let format = self.model.book(book).map(|b| b.format);
        match (found, format) {
            (FoundAgain::Same(path, measure), Some(format)) => {
                let done = self.model.relink(book, &path, format, measure, false);
                self.relinked(book, done, out);
            }
            (FoundAgain::Named(path, measure), Some(format)) => {
                let step = Step::Differs {
                    path,
                    format,
                    measure,
                };
                self.finding(book, step);
            }
            _ => {
                let name = folder_name(folder);
                let note = format!("It isn't anywhere in “{name}”.");
                self.finding(book, Step::Note(note));
            }
        }
    }

    /// Closes the sheet and opens the book, or says why not.
    fn relinked(&mut self, book: BookId, done: Result<(), RelinkError>, out: &mut Vec<Out>) {
        let note = match done {
            Ok(()) => {
                self.sheet = None;
                out.push(Out::Command(Command::OpenBook(book)));
                return;
            }
            Err(RelinkError::Removed) => {
                self.sheet = None;
                return;
            }
            Err(RelinkError::OtherBook(title)) => {
                format!("That file is “{title}”, already in your library.")
            }
            Err(RelinkError::Format { file, book }) => {
                let (file, book) = (file.label(), book.label());
                format!("That's a {file} file; this book is a {book}.")
            }
            Err(RelinkError::Differs) => "That isn't the same file.".into(),
        };
        self.finding(book, Step::Note(note));
    }
}

pub fn folder_name(folder: &Path) -> String {
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned());
    name.unwrap_or_else(|| folder.display().to_string())
}

pub fn file_name(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    name.unwrap_or_else(|| path.display().to_string())
}
