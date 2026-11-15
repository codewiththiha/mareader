//! The worker's news: what a job coming back does to the library.

use mareader_library::Opened;

use super::{News, Out, State, Status, wall_ms};
use crate::command::Command;
use crate::toast::Toast;

impl State {
    /// Takes in finished disk work.
    pub fn receive(&mut self, now: f64, out: &mut Vec<Out>) {
        for news in self.io.poll() {
            match news {
                News::Loaded(loaded) => {
                    // Opens before loading can't have touched the model.
                    let loaded = *loaded;
                    self.model = loaded.library;
                    self.status = match loaded.writable {
                        true => Status::Ready,
                        false => Status::ReadOnly,
                    };
                    let notes = loaded.notes.into_iter().map(Toast::error);
                    out.extend(notes.map(Out::Toast));
                    let held = self.waiting.drain(..);
                    out.extend(held.map(|r| Out::Command(Command::Open(r))));
                    // Gone files and new books, found where they're kept.
                    self.rescan(now);
                }
                News::Measured(probe, measure) => {
                    let opened = Opened {
                        path: probe.path,
                        measure,
                        format: probe.format,
                        title: probe.title,
                        author: probe.author,
                    };
                    self.model.record_open_as(opened, wall_ms(), probe.book);
                }
                News::WriteFailed(e) => out.push(Out::Toast(Toast::error(e))),
                News::Walked(done) => self.walked(*done, now, out),
                News::Files(done) => self.measured(*done, now, out),
                News::Copied(done) => self.copied(*done, now, out),
                News::Checked(gone) => self.checked(&gone),
                News::Relinked(relink, measured) => self.relink_measured(relink, measured, out),
                News::Sought {
                    book,
                    folder,
                    found,
                } => self.sought_found(book, &folder, found, out),
                News::Beat(beat) => self.runs.beat(beat),
            }
        }
    }
}
