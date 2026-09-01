//! Auto-center the current page in the thumbnail grid: the glide / grace /
//! debounce machinery.
//!
//! Scrolls through the panel's virtualizer — content coordinates,
//! layout-clamped — instead of hand-rolled offset arithmetic.
//!
//! Split by responsibility:
//!   * [`math`]   — the pure timing rules (`center_offset`, `glide_delay`,
//!     `glide_verdict`): named and unit-tested rather than buried in
//!     closures.
//!   * [`wiring`] — the effect and listener installation: the reveal-active
//!     gesture, the open-snap + page-follow effect, the self-re-arming
//!     glide, and the panel-lifetime cleanup.
//!
//! [`AutoCenter::install`] wires the two together.

mod math;
mod wiring;

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use crate::state::ReaderState;
use crate::state::ui::SidebarMode;

/// Panel-lifetime state shared between the thumbnail panel's effects and the
/// auto-center machinery.
pub struct AutoCenter {
    /// Last time the user physically drove the thumb panel.
    pub last_user_drive: Rc<Cell<f64>>,
    /// (was-this-panel-open, last-centered page).
    pub centered: StoredValue<(bool, u32), LocalStorage>,
    /// Handle for the debounced auto-center glide.
    pub glide_timer: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    /// The current self-re-arming glide step.
    pub glide_step: StoredValue<Option<Rc<dyn Fn()>>, LocalStorage>,
    /// The panel's virtualizer.
    pub virtualizer: Virtualizer,
}

impl AutoCenter {
    /// Create the bundle around the panel's virtualizer.
    pub fn new(virtualizer: Virtualizer) -> Self {
        Self {
            last_user_drive: Rc::new(Cell::new(f64::NEG_INFINITY)),
            centered: StoredValue::new_local((false, 0u32)),
            glide_timer: StoredValue::new_local(None::<TimeoutHandle>),
            glide_step: StoredValue::new_local(None::<Rc<dyn Fn()>>),
            virtualizer,
        }
    }

    /// Install the auto-center effects.
    pub fn install(self, state: ReaderState, sidebar: RwSignal<SidebarMode>) {
        wiring::install_reveal_listener(&self, state, sidebar);
        wiring::install_center_effect(&self, state, sidebar);
        wiring::install_lifetime_cleanup(&self);
    }
}
