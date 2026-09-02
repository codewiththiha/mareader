//! Multi-select management for gloss marks: the shared state helpers, the
//! long-press gesture constants (the gesture itself lives in
//! [`super::mark_layer`], implemented by the primitive `long_press`), the exit
//! paths (Escape / clean tap outside), the right-click context-menu
//! listener, and the undo pipeline that every removal path parks through.
//!
//! Selection state itself lives on `state.reader.gloss` so every page's
//! `GlossMarkLayer` and the reader-level bar share one source of truth.
//! Marks mutate it directly (toggling is high-frequency); only the context
//! menu travels as a CustomEvent, mirroring `GLOSS_OPEN_EVENT`.
//!
//! Dismissal (Escape / outside press) for selection mode and for the context
//! menu comes from the primitive `use_dismiss`; this module owns only the
//! semantics (exit selection, close menu, park undo).

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use ai_core::gloss::GlossMark;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::ai::gloss::controller::GlossController;
use crate::components::primitives::floating::dismiss::{use_dismiss, DismissPolicy, DismissTrigger};
use crate::components::primitives::hooks::use_custom_event::{dispatch_typed_event, use_typed_event};
use crate::state::AppState;

pub use crate::events::GLOSS_CONTEXT_EVENT;

/// How long a press must hold before it becomes a selection gesture.
pub const LONG_PRESS_MS: i32 = 450;

/// Pointer may drift this far (px) during a long-press without cancelling it.
pub const LONG_PRESS_SLOP_PX: f64 = 8.0;

/// How long the undo toast stays up before the removal is final.
pub const UNDO_WINDOW_MS: i32 = 6000;

/// Payload of [`GLOSS_CONTEXT_EVENT`]: where the menu should open and which
/// mark it acts on. Client coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextTarget {
    pub x: f64,
    pub y: f64,
    pub id: String,
}

/// A removed batch parked for undo. `path` pins the batch to the document it
/// came from: undoing after a document switch would resurrect marks into the
/// wrong file, so a stale batch is dropped instead.
#[derive(Debug, Clone)]
pub struct UndoBatch {
    pub generation: u64,
    pub path: Option<String>,
    pub marks: Vec<GlossMark>,
}

/// Monotonic batch id: the auto-dismiss timer only clears ITS batch, so a
/// quick second removal can never be eaten by the first one's timer.
static UNDO_GEN: AtomicU64 = AtomicU64::new(1);

/// Dispatch [`GLOSS_CONTEXT_EVENT`] (fired by a mark's contextmenu handler).
pub fn dispatch_gloss_context(x: f64, y: f64, id: &str) {
    dispatch_typed_event(
        GLOSS_CONTEXT_EVENT,
        &ContextTarget {
            x,
            y,
            id: id.into(),
        },
    );
}

/// Toggle one id in the selection set.
pub fn toggle_selected(selected: RwSignal<HashSet<String>>, id: &str) {
    selected.update(|s| {
        if !s.remove(id) {
            s.insert(id.to_string());
        }
    });
}

/// Exit selection mode and drop the selection.
pub fn exit_selection(state: AppState) {
    state.reader.gloss.selection_active.set(false);
    state.reader.gloss.selected_marks.set(HashSet::new());
}

/// Park a removed batch for undo and arm the auto-dismiss timer.
pub fn park_undo(
    undo: RwSignal<Option<UndoBatch>>,
    marks: Vec<GlossMark>,
    path: Option<String>,
) {
    if marks.is_empty() {
        return;
    }
    let generation = UNDO_GEN.fetch_add(1, Ordering::Relaxed);
    undo.set(Some(UndoBatch { generation, path, marks }));
}

/// Handles owned by [`use_select_mode`].
pub struct SelectMode {
    /// The right-click context menu target (client coords + mark id).
    pub menu: RwSignal<Option<ContextTarget>>,
    /// The batch currently parked for undo, if any.
    pub undo: RwSignal<Option<UndoBatch>>,
}

/// Selection-mode wiring that lives at the popover level: entry guard, exit
/// paths, the context-menu listener + dismissal, and the undo signal.
pub fn use_select_mode(state: AppState, ctrl: GlossController) -> SelectMode {
    let selecting = state.reader.gloss.selection_active;
    let menu = RwSignal::new(None::<ContextTarget>);
    let undo = RwSignal::new(None::<UndoBatch>);

    // Entering selection mode folds any open card: selection is about the
    // strokes, and the bar wants its corner of the screen to itself.
    Effect::new(move |_| {
        if selecting.get() {
            ctrl.commands.collapse_to_mark.run(());
            menu.set(None);
        }
    });

    // Escape exits selection mode; a clean tap anywhere that is not a mark,
    // the bar, or a menu exits too. (Mark clicks stop propagation;
    // drag-scrolls never synthesize clicks.)
    use_dismiss(
        selecting.into(),
        Callback::new(move |_| exit_selection(state)),
        DismissPolicy {
            escape: true,
            outside: Some(DismissTrigger::Click),
            exclude_selectors: vec![".gloss-mark", ".gloss-select-bar", ".gloss-context-menu"],
            enabled: None,
            topmost_only: false,
        },
        |_| false,
    );

    // Right-click on a mark asks for the remove menu — only outside
    // selection mode; inside it, right-click toggles selection (marks.rs).
    // Placement (cursor point) + viewport clamping + dismissal are the
    // `ContextMenu` primitive's job; this listener only delivers the payload.
    use_typed_event::<ContextTarget>(GLOSS_CONTEXT_EVENT, move |t| {
        if selecting.get_untracked() {
            return;
        }
        menu.set(Some(t));
    });

    SelectMode { menu, undo }
}
