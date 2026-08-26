//! Pointer physics for dragging the expanded card — a thin domain wrapper
//! over the primitive drag mechanics ([`use_pointer_drag`]). Writes an
//! anchor-relative offset (owned by the [`GlossController`]) so the card
//! keeps gliding with the page on scroll: target = f(live_anchor) + offset.

use leptos::prelude::*;
use pdf_core::gloss::GlossBox;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::placement::clamped_origin;
use crate::components::primitives::hooks::use_viewport::viewport_size;
use crate::components::primitives::interactions::drag::use_pointer_drag;

pub struct CardDrag {
    /// Hand to the surface's drag handle: `(client_x, client_y, box_now)`.
    pub on_drag_start: Callback<(f64, f64, GlossBox)>,
}

pub fn use_card_drag(ctrl: GlossController, expanded: Memo<Option<GlossBox>>) -> CardDrag {
    let on_drag_start = Callback::new(move |(cx, cy, origin): (f64, f64, GlossBox)| {
        ctrl.drag.grab.set_value(Some((cx - origin.x, cy - origin.y)));
        ctrl.drag.active.set(true);
    });

    use_pointer_drag(
        ctrl.drag.active,
        move |mx, my| {
            let Some((dx, dy)) = ctrl.drag.grab.get_value() else {
                return;
            };
            let Some(e) = expanded.get_untracked() else {
                return;
            };
            let (vw, vh) = viewport_size();
            // Clamp the ABSOLUTE pointer-derived origin — the same clamp
            // the spring target applies to the offset — then store the
            // offset relative to the un-dragged expanded box.
            let b = clamped_origin(e, mx - dx, my - dy, vw, vh);
            ctrl.drag.offset.set(Some((b.x - e.x, b.y - e.y)));
        },
        move || {
            ctrl.drag.grab.set_value(None);
            ctrl.drag.active.set(false);
        },
    );

    CardDrag { on_drag_start }
}
