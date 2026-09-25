//! The drag session: what is held, where the pointer is, which target is hot,
//! and what a release right now would mean.
//!
//! One controller per library page, provided by
//! `crate::features::library::page` and read by every card, row and crumb
//! under it.

use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use library_core::book::{Row, find_row};
use library_core::shelf::{ALL_SHELF, Shelf, can_nest, find};

use super::effect::{
    Band, DropEffect, DropQuery, FoldPreview, drop_effect, fold_items, fold_preview,
};
use super::target::{DropTargetId, DropTargetKind, DropTargetRegistry};
use super::{FOLD_DWELL_MS, SINK_DWELL_MS, commit};
use crate::features::library::selection::exit_selection;
use crate::state::AppState;

/// One struct rather than a book-or-folder enum, because a selection holds
/// both: every move is a pair of operations on one shelf list, so a pre-split
/// payload is one the commit step does not have to sort.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DragPayload {
    pub books: Vec<String>,
    pub folders: Vec<String>,
    /// What a move takes its books off: a drag out of an expanded branch is a
    /// move out of that shelf, and reading the open level instead would
    /// unfile a book from the shelf it was showing in.
    pub source: Option<String>,
}

impl DragPayload {
    pub fn len(&self) -> usize {
        self.books.len() + self.folders.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // Asked by every card on every repaint of a drag: a question about the
    // payload rather than a derived set of its own.
    pub fn contains(&self, id: &str) -> bool {
        self.books.iter().any(|each| each.as_str() == id)
            || self.folders.iter().any(|each| each.as_str() == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhostTile {
    pub cover: Option<String>,
    pub label: String,
    pub folder: bool,
}

/// Captured once when the dwell runs out rather than read per frame: a sunk
/// ghost means the pointer has stopped, so the target's box is not changing
/// either.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SinkSpot {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy)]
pub struct DragController {
    session: RwSignal<bool>,
    payload: RwSignal<Option<DragPayload>>,
    pointer: RwSignal<(f64, f64)>,
    hot: RwSignal<Option<DropTargetId>>,
    effect: RwSignal<Option<DropEffect>>,
    fold: RwSignal<Option<FoldPreview>>,
    sink: RwSignal<Option<SinkSpot>>,
    /// While parked, the only thing a pointermove is tested against until the
    /// pointer leaves: a held pointer on a crumb costs one comparison per move
    /// instead of a rect read for every target on the shelf.
    sink_rect: RwSignal<Option<(f64, f64, f64, f64)>>,
    dwell: RwSignal<bool>,
    /// Separate from [`Self::hot`]: the dwell's timer is an effect on this
    /// signal, and its cleanup is what clears the timer that was counting.
    dwell_target: RwSignal<Option<DropTargetId>>,
    pub registry: DropTargetRegistry,
    state: AppState,
}

impl DragController {
    pub fn install(state: AppState) -> Self {
        let this = Self {
            session: RwSignal::new(false),
            payload: RwSignal::new(None),
            pointer: RwSignal::new((0.0, 0.0)),
            hot: RwSignal::new(None),
            effect: RwSignal::new(None),
            fold: RwSignal::new(None),
            sink: RwSignal::new(None),
            sink_rect: RwSignal::new(None),
            dwell: RwSignal::new(false),
            dwell_target: RwSignal::new(None),
            registry: DropTargetRegistry::new(),
            state,
        };
        provide_context(this);
        this.bind_session();
        this.bind_dwells();
        this.bind_escape();
        this
    }

    /// Not `crate::components::primitives::interactions::drag`: that primitive
    /// finishes a drag one way; here a release and a cancellation are different
    /// answers.
    fn bind_session(&self) {
        let this = *self;
        Effect::new(move |_| {
            if !this.session.get() {
                return;
            }
            let moved = window_event_listener_untyped("pointermove", move |ev: web_sys::Event| {
                let at = ev.unchecked_ref::<web_sys::MouseEvent>();
                this.on_move(at.client_x() as f64, at.client_y() as f64);
            });
            let released = window_event_listener_untyped("pointerup", move |ev: web_sys::Event| {
                let at = ev.unchecked_ref::<web_sys::MouseEvent>();
                this.release(at.client_x() as f64, at.client_y() as f64);
            });
            let taken = window_event_listener_untyped("pointercancel", move |_| this.cancel());
            on_cleanup(move || {
                moved.remove();
                released.remove();
                taken.remove();
            });
        });
    }

    /// One timer each, both owned by an effect on the target they count
    /// against, so moving to another target or ending the drag clears them.
    /// They are not one question at two depths: the sink belongs to the title
    /// bar alone.
    fn bind_dwells(&self) {
        let this = *self;
        Effect::new(move |_| {
            let Some(target) = this.dwell_target.get() else {
                return;
            };
            let sinkable = target.0 == DropTargetKind::Shelf
                && this.effect.get_untracked().as_ref() != Some(&DropEffect::Refused);
            if sinkable && let Some(rect) = this.registry.rect_of(&target) {
                let spot = SinkSpot {
                    x: rect.left() + rect.width() / 2.0,
                    y: rect.top() + rect.height() / 2.0,
                };
                let bounds = (rect.left(), rect.top(), rect.right(), rect.bottom());
                let still = target.clone();
                if let Ok(sunk) = set_timeout_with_handle(
                    move || {
                        if this.dwell_target.get_untracked().as_ref() != Some(&still) {
                            return;
                        }
                        this.sink.set(Some(spot));
                        this.sink_rect.set(Some(bounds));
                    },
                    Duration::from_millis(SINK_DWELL_MS as u64),
                ) {
                    on_cleanup(move || sunk.clear());
                }
            }
            // The fold arms off the answer, not only the target's kind: a
            // book the session reads as a landing becomes a partner on a
            // rest, and the table answers that only for a hold with books.
            let arms_fold = matches!(
                this.effect.get_untracked(),
                Some(DropEffect::InsertBefore { .. })
            ) && target.0 == DropTargetKind::Book;
            if !arms_fold {
                return;
            }
            let still = target;
            let Ok(folded) = set_timeout_with_handle(
                move || {
                    if this.dwell_target.get_untracked().as_ref() != Some(&still) {
                        return;
                    }
                    this.dwell.set(true);
                    this.refresh();
                },
                Duration::from_millis(FOLD_DWELL_MS as u64),
            ) else {
                return;
            };
            on_cleanup(move || folded.clear());
        });
    }

    fn bind_escape(&self) {
        let this = *self;
        let handle = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            if let Ok(key) = ev.dyn_into::<web_sys::KeyboardEvent>()
                && key.key() == "Escape"
            {
                this.cancel();
            }
        });
        on_cleanup(move || handle.remove());
    }

    pub fn begin(&self, payload: DragPayload, x: f64, y: f64) {
        if payload.is_empty() || self.session.get_untracked() {
            return;
        }
        self.payload.set(Some(payload));
        self.pointer.set((x, y));
        self.session.set(true);
    }

    pub fn release(&self, x: f64, y: f64) {
        if !self.session.get_untracked() {
            return;
        }
        self.pointer.set((x, y));
        let next = self.registry.hit_test(x, y);
        if next != self.hot.get_untracked() {
            self.hot.set(next);
        }
        self.refresh();
        let effect = self.effect.get_untracked();
        let applied = effect
            .as_ref()
            .is_some_and(|each| *each != DropEffect::Refused);
        if let Some(effect) = effect {
            let payload = self.payload.get_untracked().unwrap_or_default();
            commit::apply(self.state, effect, payload);
        }
        self.end(applied);
    }

    pub fn cancel(&self) {
        if !self.session.get_untracked() {
            return;
        }
        self.end(false);
    }

    pub fn live(&self) -> Signal<bool> {
        self.session.into()
    }

    pub fn pointer(&self) -> Signal<(f64, f64)> {
        self.pointer.into()
    }

    pub fn fold(&self) -> Signal<Option<FoldPreview>> {
        self.fold.into()
    }

    pub fn sink(&self) -> Signal<Option<SinkSpot>> {
        self.sink.into()
    }

    // The visible half of a multi-drag: the picked-up set stays readable as
    // a set while the pointer carries it.
    pub fn holds(&self, id: &str) -> bool {
        self.payload
            .with(|at| at.as_ref().is_some_and(|held| held.contains(id)))
    }

    pub fn count(&self) -> Signal<usize> {
        let this = *self;
        Signal::derive(move || this.payload.get().map(|held| held.len()).unwrap_or(0))
    }

    pub fn ghost(&self) -> Signal<Vec<GhostTile>> {
        let this = *self;
        Signal::derive(move || {
            let Some(held) = this.payload.get() else {
                return Vec::new();
            };
            let state = this.state;
            let rows: Vec<Row> = state.library.books.get();
            let shelves: Vec<Shelf> = state.library.shelves.get();
            let covers = state.library.covers.get();
            let mut tiles = Vec::with_capacity(held.len());
            for id in &held.books {
                let Some(row) = find_row(&rows, id) else {
                    continue;
                };
                tiles.push(match row.book() {
                    Some(book) => GhostTile {
                        cover: covers.get(book.path()).map(|cover| cover.data_url.clone()),
                        label: book.title(),
                        folder: false,
                    },
                    None => GhostTile {
                        cover: None,
                        label: row.display_name(),
                        folder: false,
                    },
                });
            }
            for id in &held.folders {
                tiles.push(GhostTile {
                    cover: None,
                    label: find(&shelves, id)
                        .map(|each| each.name.clone())
                        .unwrap_or_default(),
                    folder: true,
                });
            }
            tiles
        })
    }

    pub fn inserts_before(&self, id: &str) -> bool {
        self.effect
            .with(|at| at.as_ref().and_then(|each| each.insert_at()) == Some((id, false)))
    }

    pub fn inserts_after(&self, id: &str) -> bool {
        self.effect
            .with(|at| at.as_ref().and_then(|each| each.insert_at()) == Some((id, true)))
    }

    pub fn sibling_before(&self, id: &str) -> bool {
        self.effect
            .with(|at| at.as_ref().and_then(|each| each.sibling_at()) == Some((id, false)))
    }

    pub fn sibling_after(&self, id: &str) -> bool {
        self.effect
            .with(|at| at.as_ref().and_then(|each| each.sibling_at()) == Some((id, true)))
    }

    pub fn folds_with(&self, id: &str) -> bool {
        self.fold.with(|at| {
            at.as_ref()
                .is_some_and(|preview| preview.with_book_id == id)
        })
    }

    pub fn nests_into(&self, id: &str) -> bool {
        self.effect
            .with(|at| at.as_ref().and_then(|each| each.nest_into()) == Some(id))
    }

    /// Asked by the breadcrumb while a drag is live: a drag cannot raise a
    /// `mouseenter`, because the pressed card holds the pointer capture.
    pub fn over_ellipsis(&self) -> bool {
        self.hot.with(|at| {
            at.as_ref()
                .is_some_and(|target| target.0 == DropTargetKind::Ellipsis)
        })
    }

    pub fn over_shelf(&self, id: &str) -> bool {
        self.hot.with(|at| {
            at.as_ref()
                .is_some_and(|target| target.0 == DropTargetKind::Shelf && target.1 == id)
        })
    }

    // The tree's hover-to-expand: the reader should not have to put the hold
    // down to knock.
    pub fn over_folder(&self, id: &str) -> bool {
        self.hot.with(|at| {
            at.as_ref()
                .is_some_and(|target| target.0 == DropTargetKind::Folder && target.1 == id)
        })
    }

    fn on_move(&self, x: f64, y: f64) {
        // Parked on a crumb, the ghost reads the target, not the hand: one
        // cached rect test and a return. The first move that leaves the box
        // resumes the follow.
        if let Some((left, top, right, bottom)) = self.sink_rect.get_untracked() {
            if x >= left && x <= right && y >= top && y <= bottom {
                return;
            }
            // Deliberately not re-arming the dwell on a target the pointer
            // never left: a cached box can go stale without the target
            // changing (a wheel scroll, a level re-laying under an import).
            self.release_sink();
        }
        self.pointer.set((x, y));
        let next = self.registry.hit_test(x, y);
        if next == self.hot.get_untracked() {
            return;
        }
        self.hot.set(next.clone());
        self.dwell.set(false);
        self.release_sink();
        self.dwell_target.set(next);
        self.refresh();
    }

    /// The spot and its cached box are one fact; clearing them apart would
    /// leave the fast path testing a box nothing is sunk in.
    fn release_sink(&self) {
        self.sink.set(None);
        self.sink_rect.set(None);
    }

    /// Every read is untracked: this runs from a pointer event and a timer,
    /// not a reactive scope, and a tracked read would subscribe whatever scope
    /// was current to the whole library.
    fn refresh(&self) {
        let Some(held) = self.payload.get_untracked() else {
            return self.clear_answer();
        };
        let Some(target) = self.hot.get_untracked() else {
            return self.clear_answer();
        };
        let open = self.state.library.shelf.get_untracked();
        let row_shelf = match self.registry.entry_of(&target).and_then(|each| each.shelf) {
            Some(named) => (named != ALL_SHELF).then_some(named),
            None => (open != ALL_SHELF).then(|| open.clone()),
        };
        let target_id = match target.0 {
            DropTargetKind::Level if open == ALL_SHELF => String::new(),
            DropTargetKind::Level => open,
            _ => target.1.clone(),
        };
        let query = DropQuery {
            held_books: held.books.len(),
            held_folders: held.folders.len(),
            target_kind: target.0,
            target_id: &target_id,
            target_is_held: held.contains(&target.1),
            can_nest: target.0 == DropTargetKind::Folder && self.can_nest_held(&held, &target.1),
            can_sibling: target.0 == DropTargetKind::Folder
                && self.can_sibling_held(&held, &target.1),
            band: self.band_of(&target),
            target_shelf: row_shelf.as_deref(),
            dwell_armed: self.dwell.get_untracked(),
        };
        let effect = drop_effect(query);
        self.fold.set(match &effect {
            DropEffect::CreateFolder { with_book_id } => {
                fold_preview(fold_items(&query), with_book_id)
            }
            _ => None,
        });
        // A brewing fold outranks the sink: the same gesture at two depths,
        // and a shrunk plate inside the card it offers to replace says
        // nothing.
        if self.fold.get_untracked().is_some() {
            self.release_sink();
        }
        self.effect.set(Some(effect));
    }

    fn clear_answer(&self) {
        self.effect.set(None);
        self.fold.set(None);
    }

    /// Asked per held shelf rather than for the batch:
    /// `library_core::shelf::can_nest` is about one tree edge, and a batch
    /// "no" would refuse a drag that could have filed two of its three
    /// folders.
    fn can_nest_held(&self, held: &DragPayload, target: &str) -> bool {
        self.state.library.shelves.with_untracked(|shelves| {
            held.folders
                .iter()
                .all(|each| can_nest(shelves, each, target))
        })
    }

    /// A root-level seam has no parent to close a loop through, so an anchor
    /// at the top only refuses a shelf asked to sibling itself.
    fn can_sibling_held(&self, held: &DragPayload, anchor: &str) -> bool {
        self.state.library.shelves.with_untracked(|shelves| {
            let Some(target) = find(shelves, anchor) else {
                return false;
            };
            held.folders.iter().all(|each| {
                each != anchor
                    && target
                        .parent
                        .as_deref()
                        .is_none_or(|parent| can_nest(shelves, each, parent))
            })
        })
    }

    /// Computed here rather than in the table or the rows: the seam a row
    /// paints, the effect the table answers and the index the commit resolves
    /// must agree, and one rectangle read in one place keeps them agreed.
    fn band_of(&self, target: &DropTargetId) -> Band {
        if !self.state.library.view.with_untracked(|v| v.is_list()) {
            return Band::Middle;
        }
        let Some(rect) = self.registry.rect_of(target) else {
            return Band::Middle;
        };
        let (_, y) = self.pointer.get_untracked();
        let at = (y - rect.top()) / rect.height().max(1.0);
        match target.0 {
            DropTargetKind::Folder => {
                if at < 0.25 {
                    Band::Top
                } else if at > 0.75 {
                    Band::Bottom
                } else {
                    Band::Middle
                }
            }
            _ => {
                if at < 0.5 {
                    Band::Top
                } else {
                    Band::Bottom
                }
            }
        }
    }

    /// The one place a drag stops being a drag: listeners go with the effect
    /// that owns them, the dwell timer with its own effect, and everything the
    /// cards paint from is written back to nothing in the same breath.
    fn end(&self, applied: bool) {
        self.session.set(false);
        self.payload.set(None);
        self.pointer.set((0.0, 0.0));
        self.hot.set(None);
        self.dwell.set(false);
        self.dwell_target.set(None);
        self.release_sink();
        self.clear_answer();
        if applied && self.state.library.selecting.get_untracked() {
            exit_selection(self.state);
        }
    }
}
