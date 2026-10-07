//! Pointer physics for dragging the expanded card.

use ai_core::gloss::GlossBox;
use leptos::prelude::*;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::placement::clamped_origin;
use app_chrome::hooks::use_viewport::viewport_size;
use app_ui::components::primitives::interactions::drag::use_pointer_drag;

pub struct CardDrag {
    /// Hand to the surface's drag handle: `(client_x, client_y, box_now)`.
    pub on_drag_start: Callback<(f64, f64, GlossBox)>,
}

pub fn use_card_drag(ctrl: GlossController, expanded: Memo<Option<GlossBox>>) -> CardDrag {
    let on_drag_start = Callback::new(move |(cx, cy, origin): (f64, f64, GlossBox)| {
        ctrl.drag
            .grab
            .set_value(Some((cx - origin.x, cy - origin.y)));
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
            // Clamp the pointer origin, then store the relative offset.
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
