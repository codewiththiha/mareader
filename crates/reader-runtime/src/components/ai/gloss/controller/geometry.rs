//! Where the card's box is in its morph lifecycle.

use leptos::prelude::*;

use crate::components::ai::gloss::phase::GlossPhase;

/// The geometry phase of the surface: where the card's *box* is in its
/// morph lifecycle, and whether the surface exists at all.
#[derive(Clone, Copy)]
pub struct GlossGeometry {
    pub gphase: RwSignal<GlossPhase>,
    /// Whether the morphing surface exists at all. Distinct from
    /// `popover_open`: during processing the stroke IS the UI, and after the
    /// outro morph the surface unmounts while the gloss stays "open" on its
    /// mark.
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
