//! Pane tree, focus recency, and route state inside [`Session`].

use mareader_workspace::{Bounds, Edge, MAX_PANES, PaneId, TreeError};

use crate::event::Event;
use crate::route::Route;
use crate::toast::Toast;

use super::{Done, Session};

impl Session {
    /// Makes `id` the focused pane, newest in the recency list.
    pub(crate) fn focus(&mut self, id: PaneId) {
        if !self.tree.panes().contains(&id) {
            return;
        }
        self.active = id;
        self.restage();
    }

    /// Puts the focused pane first in `recent`, dropping closed panes.
    fn restage(&mut self) {
        let placed = self.tree.panes();
        let active = self.active;
        self.recent.retain(|p| placed.contains(p) && *p != active);
        self.recent.insert(0, active);
        let unseen: Vec<PaneId> = placed
            .into_iter()
            .filter(|p| !self.recent.contains(p))
            .collect();
        self.recent.extend(unseen);
    }

    /// The page the window shows; only the Dictionary remembers where from.
    pub(crate) fn show(&mut self, route: Route) {
        let here = self.route;
        self.dict_from = match route {
            Route::Dictionary if here == Route::Dictionary => self.dict_from,
            Route::Dictionary => Some(here),
            _ => None,
        };
        self.route = route;
        if route != Route::Dictionary {
            self.library.showing = route == Route::Library;
        }
        if route != here || route == Route::Dictionary {
            self.emit(Event::RouteChanged(route));
        }
    }

    /// A new pane on `edge` of `pane`, in `area`; a refusal is a note.
    ///
    /// The geometry is the window's, so it arrives as plain numbers.
    pub(crate) fn split_at(&mut self, pane: PaneId, edge: Edge, area: Bounds) -> Option<PaneId> {
        let (axis, side) = edge.placement();
        let why = match self.tree.split(pane, axis, side, area) {
            Ok(id) => return Some(id),
            Err(TreeError::Full) => format!("A workspace holds {MAX_PANES} panes at most."),
            Err(_) => "This pane is too small to split.".to_owned(),
        };
        self.emit(Event::Toast(Toast::warning(why)));
        None
    }

    /// Right of `pane`, else below it when the right has no room.
    pub fn beside(&self, pane: PaneId, area: Bounds) -> Edge {
        let mut probe = self.tree.clone();
        let (axis, side) = Edge::Right.placement();
        match probe.split(pane, axis, side, area) {
            Err(TreeError::NoRoom(_)) => Edge::Bottom,
            _ => Edge::Right,
        }
    }

    pub(crate) fn close(&mut self, id: PaneId) -> bool {
        if self.tree.close(id).is_err() {
            return false;
        }
        self.recent.retain(|p| *p != id);
        self.page_ai.close_pane(id);
        self.skills.keep_pane(id, None);
        if self.active == id {
            let next = self.tree.panes().first().copied().unwrap_or(PaneId(1));
            self.active = next;
        }
        true
    }

    /// Trades two panes' places in the tree.
    pub(super) fn swap(&mut self, pane: PaneId, other: PaneId) -> Done {
        match self.tree.swap(pane, other) {
            Ok(()) => Done::changed(),
            Err(_) => self.no_room(),
        }
    }

    /// Moves `pane` to `other`'s `edge`, measured against the window's `area`.
    pub(super) fn move_to(
        &mut self,
        pane: PaneId,
        other: PaneId,
        edge: Edge,
        area: Bounds,
    ) -> Done {
        match self.tree.move_to(pane, other, edge, area) {
            Ok(()) => Done::changed(),
            Err(_) => self.no_room(),
        }
    }

    /// A pane surgery the tree has no room for: the reader is told.
    fn no_room(&mut self) -> Done {
        let why = "There's no room for the pane there.";
        self.emit(Event::Toast(Toast::warning(why)));
        Done::quiet()
    }
}
