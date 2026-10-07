//! Multi-select for gloss marks: helpers, gestures, exits, undo.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use ai_core::gloss::GlossMark;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::ai::gloss::controller::GlossController;
use crate::pane::origin::raised_in;
use app_chrome::floating::dismiss::{DismissPolicy, DismissTrigger, use_dismiss};
use app_ui::components::primitives::hooks::use_custom_event::use_typed_event_from;
use app_ui::events::dispatch_typed_event_on;

pub use app_ui::events::GLOSS_CONTEXT_EVENT;

/// How long a press must hold, and how far it may drift.
pub use app_ui::components::primitives::interactions::long_press::{
    SELECT_PRESS_MS as LONG_PRESS_MS, SELECT_SLOP_PX as LONG_PRESS_SLOP_PX,
};

/// How long the undo toast stays up before the removal is final.
pub const UNDO_WINDOW_MS: i32 = 6000;

/// The context-menu event's payload: where, and which mark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextTarget {
    pub x: f64,
    pub y: f64,
    pub id: String,
}

/// A removed batch parked for undo, pinned to its document.
#[derive(Debug, Clone)]
pub struct UndoBatch {
    pub generation: u64,
    pub path: Option<String>,
    pub marks: Vec<GlossMark>,
}

/// Monotonic batch id, so only ITS batch's timer clears it.
static UNDO_GEN: AtomicU64 = AtomicU64::new(1);

/// Dispatch the context event ON the mark, bubbling.
pub fn dispatch_gloss_context(origin: &web_sys::EventTarget, x: f64, y: f64, id: &str) {
    dispatch_typed_event_on(
        origin,
        GLOSS_CONTEXT_EVENT,
        &ContextTarget {
            x,
            y,
            id: id.into(),
        },
    );
}

pub fn toggle_selected(selected: RwSignal<HashSet<String>>, id: &str) {
    selected.update(|s| {
        if !s.remove(id) {
            s.insert(id.to_string());
        }
    });
}

/// Exit selection mode and drop the selection.
pub fn exit_selection(state: crate::context::ReaderContext) {
    state.reader.gloss.selection_active.set(false);
    state.reader.gloss.selected_marks.set(HashSet::new());
}

/// Park a removed batch for undo; an empty batch parks nothing.
pub fn park_undo(undo: RwSignal<Option<UndoBatch>>, marks: Vec<GlossMark>, path: Option<String>) {
    if marks.is_empty() {
        return;
    }
    let generation = UNDO_GEN.fetch_add(1, Ordering::Relaxed);
    undo.set(Some(UndoBatch {
        generation,
        path,
        marks,
    }));
}

/// Handles owned by [`use_select_mode`].
pub struct SelectMode {
    /// The right-click context menu target (client coords + mark id).
    pub menu: RwSignal<Option<ContextTarget>>,
    /// The batch currently parked for undo, if any.
    pub undo: RwSignal<Option<UndoBatch>>,
}

/// The popover-level selection wiring: guard, exits, undo.
pub fn use_select_mode(state: crate::context::ReaderContext, ctrl: GlossController) -> SelectMode {
    let selecting = state.reader.gloss.selection_active;
    let menu = RwSignal::new(None::<ContextTarget>);
    let undo = RwSignal::new(None::<UndoBatch>);

    // Entering folds any open card.
    Effect::new(move |_| {
        if selecting.get() {
            ctrl.commands.collapse_to_mark.run(());
            menu.set(None);
        }
    });

    // Escape, or a clean tap elsewhere, exits.
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

    // Right-click asks for the remove menu, outside selection mode.
    use_typed_event_from::<ContextTarget>(GLOSS_CONTEXT_EVENT, move |t, origin| {
        if selecting.get_untracked() || !raised_in(&state.reader.dom, origin.as_ref()) {
            return;
        }
        menu.set(Some(t));
    });

    SelectMode { menu, undo }
}
