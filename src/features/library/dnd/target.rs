//! What a drop can land on, and how the pointer finds it.
//!
//! Targets register themselves instead of being discovered from the event that happens to bubble
//! past: a `dragover`/`dragleave` pair counts child boundaries rather than targets, which is how
//! the shelf's old marker flickered between a card and the grid it sits in.

use leptos::prelude::*;

use app_chrome::hooks::dom::by_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTargetKind {
    Book,
    Folder,
    /// The way back to a level is therefore also a way to file whatever is held onto that level from anywhere in the library.
    Shelf,
    /// A target and not a drop: resting on it during a drag opens the panel, because a captured pointer raises no `mouseenter` for the bar to hear.
    Ellipsis,
    Level,
}

/// Never a path, for the reason `crate::services::library::arrange` gives: an in-app move edits a list of ids and never touches the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropTargetId(pub DropTargetKind, pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropTargetEntry {
    pub id: DropTargetId,
    /// Named rather than held: a card that unmounted mid-drag leaves an id that finds nothing, which is a target that cannot be hit.
    pub dom_id: String,
    /// `Some(ALL_SHELF)` spells the library's own order; `None` is "unspecified", and the session resolves it to the open level — the answer a grid card implies, because a card is only ever drawn by the shelf the page is on.
    pub shelf: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Registered {
    token: u64,
    entry: DropTargetEntry,
}

/// One per page and shared by every card: exactly one target is hot at a time, and no card has to know about any other for that to hold.
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

    /// The cleanup is the reason this is a method and not a write to a signal: a card that unmounted without leaving the registry would keep a dead id in it forever. It removes THIS registration by token, so a re-created card does not evict the newer one.
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

    /// One `elementFromPoint` and a walk up from what it found, rather than a rect read for
    /// every registered target on every pointermove: a two-hundred-card grid was two hundred
    /// forced layout reads per mouse event, each one a chance to invalidate the layout the
    /// next one read. The walk up finds the nearest registered ancestor, which is the same
    /// "card before the level it sits in" the reverse-registration order used to give — the
    /// card is a DOM descendant of the level, so the pointer's deepest element reaches it
    /// first — and among targets sharing one node the reverse order still decides.
    pub fn hit_test(&self, x: f64, y: f64) -> Option<DropTargetId> {
        let hit = web_sys::window()?.document()?.element_from_point(x, y)?;
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

    /// A sunk ghost sits at the CENTRE of the thing it is landing on, and the centre has to be read rather than remembered, because the shelf can scroll and a level can re-lay itself out between the moment a drag starts and the moment it rests somewhere.
    pub fn rect_of(&self, id: &DropTargetId) -> Option<web_sys::DomRect> {
        self.entries.with_untracked(|list| {
            list.iter()
                .find(|each| &each.entry.id == id)
                .and_then(|each| by_id(&each.entry.dom_id))
                .map(|node| node.get_bounding_client_rect())
        })
    }

    /// What the session asks for a row's own container: the hit-test answers WHICH target the pointer is on, and the entry carries the shelf whose member list renders it.
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
