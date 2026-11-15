//! What a walk's news lands: the books it found, the copies they owed, and the
//! words for what changed.
//!
//! The worker walks and copies; this is what comes back, so a landing lands on
//! a model that remembers the walk it came from.

use std::collections::HashMap;
use std::path::PathBuf;

use mareader_library::{BookId, ContentId, Copied, FolderId, Found, Landed, Planned, ShelfId};

use super::{
    Copies, CopyIn, Files, Going, Job, Measured, Out, Pending, Phase, Pick, Question, RunId,
    SETTLE_WAIT, Screening, Sheet, State, Walked, file_name, folder_name, wall_ms,
};
use crate::toast::Toast;

impl State {
    pub fn walked(&mut self, done: Walked, now: f64, out: &mut Vec<Out>) {
        let Some(Pending::Walk(mut going, folder)) = self.folders.pending.remove(&done.tag) else {
            return;
        };
        let name = folder_name(&going.root);
        if done.settling {
            self.folders.settle_at = Some(now + SETTLE_WAIT);
        }
        if let Some(id) = folder {
            self.rescanned(id, done, now);
            return;
        }
        if done.offline {
            self.runs.finish(going.run);
            let note = format!("“{name}” couldn't be read.");
            out.push(Out::Toast(Toast::error(note)));
            return;
        }
        if done.truncated {
            let note = format!("“{name}” is very large; only part of it was read.");
            out.push(Out::Toast(Toast::info(note)));
        }
        going.found = done.found.len();
        let wall = wall_ms();
        let (root, opts) = (going.root.clone(), going.opts.clone());
        let id = self.model.open_folder(root, opts, &going.plan, wall);
        let plan = going.plan.clone();
        let Some(planned) = self.model.plan_walk(id, done.found, going.asked, plan) else {
            self.runs.finish(going.run);
            return;
        };
        self.land_or_copy(planned, going, now, out);
    }

    /// Copies first if the folder copies; else lands at once.
    fn land_or_copy(&mut self, planned: Planned, going: Going, now: f64, out: &mut Vec<Out>) {
        let copies = self
            .model
            .folder(planned.folder)
            .is_some_and(|f| f.mode().copies());
        let files: Vec<Found> = planned.to_copy(copies).into_iter().cloned().collect();
        let store = self.store();
        match (files.is_empty(), store) {
            (false, Some(store)) => {
                let run = going.run;
                self.runs.phase(run, Phase::Copying);
                let tag = self.tag(Pending::Land(Box::new(planned), going));
                self.io.send(Job::Copy(Box::new(CopyIn {
                    tag,
                    run,
                    store,
                    files,
                })));
            }
            _ => self.land(planned, &HashMap::new(), going, now, out),
        }
    }

    fn land(
        &mut self,
        planned: Planned,
        copies: &HashMap<PathBuf, Copied>,
        going: Going,
        now: f64,
        out: &mut Vec<Out>,
    ) {
        let asks = planned.asks.clone();
        let folder = planned.folder;
        let landed = self.model.land_walk(planned, copies, wall_ms());
        self.runs.finish(going.run);
        let waiting = asks.len();
        self.asks.extend(asks.into_iter().map(Question::Merge));
        let name =
            (self.model.folder(folder)).map_or_else(|| folder_name(&going.root), |f| f.name());
        let plan = &going.plan;
        let nothing = landed.total() == 0 && waiting == 0;
        let note = match (plan.fold.is_some(), plan.continuation) {
            (true, _) => landed.root.map(|shelf| (shelf, true)),
            (false, Some(shelf)) if nothing => Some((shelf, false)),
            _ => None,
        };
        match note.filter(|_| self.sheet.is_none()) {
            Some((shelf, returned)) => self.sheet = Some(Sheet::Note { shelf, returned }),
            None => {
                let note = landed_note(&name, &landed, going.found, waiting);
                out.push(Out::Toast(Toast::info(note)));
                if let Some(shelf) = landed.root {
                    self.reveal(Pick::Shelf(shelf), now);
                }
            }
        }
    }

    pub fn copied(&mut self, done: Copies, now: f64, out: &mut Vec<Out>) {
        if let Some((path, why)) = done.failed.first() {
            let more = match done.failed.len() {
                1 => String::new(),
                n => format!(" ({} more failed)", n - 1),
            };
            let note = format!("Couldn't copy “{}”: {why}{more}.", file_name(path));
            out.push(Out::Toast(Toast::error(note)));
        }
        let copies = &done.copies;
        match self.folders.pending.remove(&done.tag) {
            Some(Pending::Land(planned, going)) => {
                self.land(*planned, &done.copies, going, now, out);
            }
            Some(Pending::Loose(loose)) => self.land_loose(*loose, &done.copies, now, out),
            Some(Pending::Keep(question, answer)) => {
                let copy = question.found().and_then(|f| done.copies.get(&f.path));
                self.write_answer(*question, answer, copy, now, out);
            }
            Some(Pending::Depart(work)) => self.departed(work, copies, out),
            Some(Pending::Duplicate(picks)) => self.duplicated(&picks, &done.copies, out),
            Some(Pending::Restored(folder, content, found)) => {
                let copy = done.copies.get(&found.path);
                self.restored(folder, content, &found, copy, now, out)
            }
            _ => {}
        }
    }

    pub fn measured(&mut self, done: Measured, now: f64, out: &mut Vec<Out>) {
        match self.folders.pending.remove(&done.tag) {
            Some(Pending::Files(to, run)) => self.screen(done, to, run, now, out),
            Some(Pending::Restore(folder, content)) => {
                self.restore_measured(folder, content, done, now, out);
            }
            _ => {}
        }
    }

    /// Screens loose files: covered and held ones ask, the rest copy.
    fn screen(
        &mut self,
        done: Measured,
        to: Option<ShelfId>,
        run: Option<RunId>,
        now: f64,
        out: &mut Vec<Out>,
    ) {
        let to = to.filter(|s| self.model.shelf(*s).is_some());
        let mut notes = Vec::new();
        match done.skipped.as_slice() {
            [] => {}
            [path] => notes.push(format!(
                "“{}” isn't a book Mareader reads.",
                file_name(path)
            )),
            many => notes.push(format!("{} files aren't books Mareader reads.", many.len())),
        }
        if let Some((path, why)) = done.failed.first() {
            notes.push(format!("Couldn't read “{}”: {why}.", file_name(path)));
        }
        let screened = self.model.screen_files(done.found, to);
        let asked = screened.asks.len();
        let asks = screened.asks.into_iter().map(Question::File);
        self.asks.extend(asks);
        if !notes.is_empty() {
            out.push(Out::Toast(Toast::error(notes.join(" "))));
        }
        let loose = Screening {
            to,
            run,
            files: screened.clean,
            asked,
        };
        match (loose.files.is_empty(), self.store()) {
            (false, Some(store)) => {
                self.runs.phase(run, Phase::Copying);
                let files = loose.files.clone();
                let tag = self.tag(Pending::Loose(Box::new(loose)));
                self.io.send(Job::Copy(Box::new(CopyIn {
                    tag,
                    run,
                    store,
                    files,
                })));
            }
            _ => self.land_loose(loose, &HashMap::new(), now, out),
        }
    }

    fn land_loose(
        &mut self,
        loose: Screening,
        copies: &HashMap<PathBuf, Copied>,
        now: f64,
        out: &mut Vec<Out>,
    ) {
        self.runs.finish(loose.run);
        let to = loose.to.filter(|s| self.model.shelf(*s).is_some());
        let pairs: Vec<(Found, Copied)> = (loose.files.into_iter())
            .filter_map(|f| copies.get(&f.path).cloned().map(|c| (f, c)))
            .collect();
        let landed = self.model.land_copies(&pairs, to, wall_ms());
        let title = |id: &BookId| self.model.book(*id).map(|b| b.title.value.clone());
        let onto = (to.and_then(|s| self.model.shelf(s)))
            .map(|s| format!(" to “{}”", s.name.value))
            .unwrap_or_default();
        let mut parts = Vec::new();
        match landed.as_slice() {
            [] => {}
            [id] => parts.push(format!(
                "Imported “{}”{onto}.",
                title(id).unwrap_or_default()
            )),
            many => parts.push(format!("Imported {} books{onto}.", many.len())),
        }
        match loose.asked {
            0 => {}
            1 => parts.push("1 waits for your choice.".to_owned()),
            n => parts.push(format!("{n} wait for your choice.")),
        }
        if !parts.is_empty() {
            out.push(Out::Toast(Toast::info(parts.join(" "))));
        }
        if let [id] = landed.as_slice() {
            self.reveal(Pick::Book(*id), now);
        }
    }

    /// A removed book back: its file looked at, then copied or read.
    pub fn restore(&mut self, folder: FolderId, content: ContentId, out: &mut Vec<Out>) {
        let stone = (self.model.folder(folder)).and_then(|f| f.tombstone(content));
        let Some(path) = stone.map(|s| s.last_path.clone()) else {
            return;
        };
        if !path.is_file() {
            let note = format!("“{}” is not there any more.", file_name(&path));
            out.push(Out::Toast(Toast::error(note)));
            return;
        }
        let tag = self.tag(Pending::Restore(folder, content));
        let paths = vec![path];
        let run = None;
        self.io
            .send(Job::Files(Box::new(Files { tag, run, paths })));
    }

    fn restore_measured(
        &mut self,
        folder: FolderId,
        content: ContentId,
        done: Measured,
        now: f64,
        out: &mut Vec<Out>,
    ) {
        let Some(found) = done.found.into_iter().next() else {
            let path = done.skipped.first().or(done.failed.first().map(|f| &f.0));
            let name = path.map(|p| file_name(p)).unwrap_or_default();
            let note = format!("“{name}” is not there any more.");
            out.push(Out::Toast(Toast::error(note)));
            return;
        };
        let copies = self.model.folder(folder).is_some_and(|f| f.mode().copies());
        match (copies, self.store()) {
            (true, Some(store)) => {
                let files = vec![found.clone()];
                let pending = Pending::Restored(folder, content, Box::new(found));
                let tag = self.tag(pending);
                let run = None;
                self.io.send(Job::Copy(Box::new(CopyIn {
                    tag,
                    run,
                    store,
                    files,
                })));
            }
            _ => self.restored(folder, content, &found, None, now, out),
        }
    }

    fn restored(
        &mut self,
        folder: FolderId,
        content: ContentId,
        found: &Found,
        copy: Option<&Copied>,
        now: f64,
        out: &mut Vec<Out>,
    ) {
        let wall = wall_ms();
        let Some(id) = (self.model).restore_removed(folder, content, found, copy, wall) else {
            return;
        };
        let title = self.model.book(id).map(|b| b.title.value.clone());
        let note = format!("Restored “{}”.", title.unwrap_or_default());
        out.push(Out::Toast(Toast::info(note)));
        self.reveal(Pick::Book(id), now);
    }
}

/// "Added 3 books from “Essays”.", or why nothing was.
fn landed_note(name: &str, landed: &Landed, found: usize, waiting: usize) -> String {
    let said = match (landed.total(), found) {
        (0, 0) => format!("No books found in “{name}”."),
        (0, _) => format!("Nothing new to import from “{name}”."),
        (1, _) => format!("Added 1 book from “{name}”."),
        (n, _) => format!("Added {n} books from “{name}”."),
    };
    match waiting {
        0 => said,
        1 => format!("{said} 1 waits for your choice."),
        n => format!("{said} {n} wait for your choice."),
    }
}
