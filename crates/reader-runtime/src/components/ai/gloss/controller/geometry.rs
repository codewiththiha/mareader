//! Where the card's box is in its morph lifecycle.

use leptos::prelude::*;

use crate::components::ai::gloss::phase::GlossPhase;

/// Where the card's box is in its morph lifecycle.
#[derive(Clone, Copy)]
pub struct GlossGeometry {
    pub gphase: RwSignal<GlossPhase>,
    /// Whether the surface exists at all.
    pub surface_visible: RwSignal<bool>,
}

impl GlossGeometry {
    pub(super) fn new() -> Self {
        Self {
            gphase: RwSignal::new(GlossPhase::Processing),
            surface_visible: RwSignal::new(false),
        }
    }

    /// Back to the pre-open state: no surface, hugging the stroke.
    pub(super) fn clear(&self) {
        self.gphase.set(GlossPhase::Processing);
        self.surface_visible.set(false);
    }
}
