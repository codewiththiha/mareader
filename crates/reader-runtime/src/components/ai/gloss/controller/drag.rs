//! Pointer state for the expanded card. The physics live in
//! [`super::super::drag`]; this is only what they read and write.

use leptos::prelude::*;

/// Drag state for the expanded card (the pointer physics live in
/// [`super::drag`]).
#[derive(Clone, Copy)]
pub struct GlossDrag {
    /// Anchor-relative offset of the dragged card (None = not dragged).
    pub offset: RwSignal<Option<(f64, f64)>>,
    /// Whether a drag is in progress (snaps the spring while true).
    pub active: RwSignal<bool>,
    /// Grab offset within the card, live only during a drag.
    pub grab: StoredValue<Option<(f64, f64)>, LocalStorage>,
}

impl GlossDrag {
    pub(super) fn new() -> Self {
        Self {
            offset: RwSignal::new(None::<(f64, f64)>),
            active: RwSignal::new(false),
            grab: StoredValue::new_local(None::<(f64, f64)>),
        }
    }

    /// Forget any drag: no offset, not dragging, no grab point.
    pub(super) fn clear(&self) {
        self.offset.set(None);
        self.active.set(false);
        self.grab.set_value(None);
    }
}
