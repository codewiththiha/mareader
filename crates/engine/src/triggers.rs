//! A book's triggers: what they show in a pane, and the clips they play.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Weak};

use mareader_audio::{Player, Segment};
use mareader_epub::{Action, Book, Insert, Triggered};
use mareader_session::Document;

use crate::host::Shared;
use crate::play::fetch;
use crate::scripts::Paged;
use crate::stable_hash::hash;

/// A clip a trigger acts on, waiting for the window to act.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipAction {
    pub action: Action,
    pub path: String,
    pub video: bool,
}

/// What triggers have shown and hidden in one pane's book.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Triggers {
    shown: BTreeMap<usize, bool>,
    /// The blocks laid for `shown`, under its key.
    made: Option<(u64, Vec<Arc<Insert>>)>,
    /// Clips clicked since the window last looked.
    pub clips: Vec<ClipAction>,
    /// What the book's scripts made of its scripted chapters.
    pub script: Paged,
}

impl Triggers {
    /// A click on a trigger link: what it sets off, in order.
    pub fn click(&mut self, all: Vec<Triggered>) {
        for t in all {
            match t {
                Triggered::Show(target, visible) => {
                    self.shown.insert(target, visible);
                }
                Triggered::Clip {
                    action,
                    path,
                    video,
                } => self.clips.push(ClipAction {
                    action,
                    path,
                    video,
                }),
            }
        }
    }

    /// Shows or hides `target`, as a click would.
    pub fn set(&mut self, target: usize, visible: bool) {
        self.shown.insert(target, visible);
    }

    /// The blocks `book` lays for what is shown, and a key; none if untouched.
    pub fn inserts(&mut self, book: &Book) -> Option<(u64, Vec<Arc<Insert>>)> {
        let pages = &self.script.pages;
        if self.shown.is_empty() && pages.is_empty() {
            return None;
        }
        let key = hash(("triggers", &self.shown, self.script.version)).max(1);
        if let Some(made) = self.made.as_ref().filter(|m| m.0 == key) {
            return Some(made.clone());
        }
        let list: Vec<_> = book
            .relaid(&self.shown, pages)
            .into_iter()
            .map(Arc::new)
            .collect();
        self.made = Some((key, list.clone()));
        Some((key, list))
    }
}

/// The clip a trigger plays now, and the book it is from.
struct Playing {
    doc: Weak<Document>,
    path: String,
    player: Player,
}

/// Clips triggers play in the window: one sounds at a time.
#[derive(Default)]
pub struct Clips {
    playing: Option<Playing>,
    /// Clips a trigger muted, by path; they stay muted when played again.
    muted: HashSet<String>,
}

impl Clips {
    /// Carries out `clip` from `doc`; true asks the window to pause narration.
    pub fn act(&mut self, host: &Shared, doc: &Arc<Document>, clip: ClipAction) -> bool {
        let ClipAction {
            action,
            path,
            video,
        } = clip;
        if video {
            if action == Action::Play {
                crate::play::play(host, Arc::clone(doc), path);
            }
            return false;
        }
        let here = (self.playing.as_ref())
            .filter(|p| p.path == path && p.doc.upgrade().is_some_and(|d| Arc::ptr_eq(&d, doc)));
        let going = here.is_some_and(|p| !p.player.status().done);
        match action {
            Action::Play | Action::Show => self.start(doc, path),
            Action::Resume if going => {
                if let Some(p) = here {
                    p.player.resume();
                }
                true
            }
            Action::Resume => self.start(doc, path),
            Action::Pause => {
                if let Some(p) = here {
                    p.player.pause();
                }
                false
            }
            Action::Mute | Action::Unmute | Action::Hide => {
                let muted = action == Action::Mute;
                if let Some(p) = here {
                    p.player.set_muted(muted);
                }
                match muted {
                    true => self.muted.insert(path),
                    false => self.muted.remove(&path),
                };
                false
            }
        }
    }

    /// Plays `path` from its start; true asks the window to pause narration.
    fn start(&mut self, doc: &Arc<Document>, path: String) -> bool {
        let same = (self.playing.as_ref())
            .is_some_and(|p| p.doc.upgrade().is_some_and(|d| Arc::ptr_eq(&d, doc)));
        if !same {
            let Some(files) = doc.files() else {
                return false;
            };
            self.playing = Some(Playing {
                doc: Arc::downgrade(doc),
                path: path.clone(),
                player: Player::new(fetch(files)),
            });
        }
        let Some(p) = self.playing.as_mut() else {
            return false;
        };
        p.player.set_muted(self.muted.contains(&path));
        p.player.play(vec![Segment {
            path: path.clone(),
            from: 0.0,
            until: None,
            tag: 0,
        }]);
        p.path = path;
        true
    }

    /// Silences the clip playing, as read-aloud starts.
    pub fn stop(&mut self) {
        self.playing = None;
    }

    /// Ends a clip whose book has closed.
    pub fn tick(&mut self) {
        if self
            .playing
            .as_ref()
            .is_some_and(|p| p.doc.strong_count() == 0)
        {
            self.playing = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_show_and_hide_and_queue_clips() {
        let mut t = Triggers::default();
        let chime = Triggered::Clip {
            action: Action::Play,
            path: "/a.mp3".to_owned(),
            video: false,
        };
        t.click(vec![Triggered::Show(2, true), chime]);
        t.click(vec![Triggered::Show(3, false), Triggered::Show(2, false)]);
        assert_eq!(t.shown, BTreeMap::from([(2, false), (3, false)]));
        assert_eq!(t.clips.len(), 1);
        assert!(t.clips.first().is_some_and(|c| c.action == Action::Play));
    }
}
