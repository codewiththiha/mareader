//! What a drop lands on, and how the pointer finds it: registered
//! targets, not events.

use leptos::prelude::*;

use app_chrome::hooks::dom::by_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTargetKind {
    Book,
    Folder,
    /// The way back to a level, and a way to file onto it from anywhere.
    Shelf,
    /// A hover target, not a drop: a captured pointer raises no `mouseenter`.
    Ellipsis,
    Level,
}

/// Never a path: an in-app move edits ids and never touches the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropTargetId(pub DropTargetKind, pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropTargetEntry {
    pub id: DropTargetId,
    /// Named, not held: an unmounted card leaves an unhittable id.
    pub dom_id: String,
    /// `Some(ALL_SHELF)` is the library's own order; `None` resolves to the
    /// open level.
    pub shelf: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Registered {
    token: u64,
    entry: DropTargetEntry,
}

/// One per page, shared by every card: exactly one target is hot at a time.
#[derive(Clone, Copy)]
pub struct DropTargetRegistry {
    entries: RwSignal<Vec<Registered>>,
    tokens: RwSignal<u64>,
}

impl DropTargetRegistry {
    pub fn new() -> Self {
        Self {
            entries: RwSignal::new(Vec::new()),
            tokens: RwSignal::new(0),
        }
    }

    /// Removes the entry on cleanup by token: a dead card cannot evict a
    /// live one.
    pub fn register(&self, entry: DropTargetEntry) {
        let id = entry.id.clone();
        self.tokens.update(|at| *at += 1);
        let token = self.tokens.get_untracked();
        self.entries.update(|list| {
            list.retain(|each| each.entry.id != id);
            list.push(Registered { token, entry });
        });
        let entries = self.entries;
        on_cleanup(move || entries.update(|list| list.retain(|each| each.token != token)));
    }

    /// One `elementFromPoint` and a walk up, not a rect read per target
    /// per move.
    pub fn hit_test(&self, x: f64, y: f64) -> Option<DropTargetId> {
        let hit = web_sys::window()?
            .document()?
            .element_from_point(x as f32, y as f32)?;
        self.entries.with_untracked(|list| {
            let mut node: Option<web_sys::Element> = Some(hit);
            while let Some(el) = node {
                let dom_id = el.id();
                if let Some(each) = list.iter().rev().find(|each| each.entry.dom_id == dom_id) {
                    return Some(each.entry.id.clone());
                }
                node = el.parent_element();
            }
            None
        })
    }

    /// Read, not remembered: the shelf can scroll between the drag and
    /// the sink.
    pub fn rect_of(&self, id: &DropTargetId) -> Option<web_sys::DomRect> {
        self.entries.with_untracked(|list| {
            list.iter()
                .find(|each| &each.entry.id == id)
                .and_then(|each| by_id(&each.entry.dom_id))
                .map(|node| node.get_bounding_client_rect())
        })
    }

    /// The entry behind a hit, carrying the shelf whose member list renders it.
    pub fn entry_of(&self, id: &DropTargetId) -> Option<DropTargetEntry> {
        self.entries.with_untracked(|list| {
            list.iter()
                .find(|each| &each.entry.id == id)
                .map(|each| each.entry.clone())
        })
    }
}

impl Default for DropTargetRegistry {
    fn default() -> Self {
        Self::new()
    }
}
