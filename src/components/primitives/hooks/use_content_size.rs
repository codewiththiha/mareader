//! Measure the real content height of an invisible "twin" node so a floating
//! card can size itself to its content (no minimum: a two-line answer gets a
//! two-line card, and content growing the twin later animates the card open a
//! little further). The twin must be a pixel-exact replica of the card's
//! scroll column.

use leptos::{html, prelude::*};


/// Returns the live height signal fed by the measure twin.
///
/// `read` is invoked inside the tracking effect, so callers express which
/// signals should trigger a re-measure by simply reading them there (e.g.
/// `move || { let _ = info.get(); let _ = word.get(); }`).
pub fn use_content_size(measure_ref: NodeRef<html::Div>, read: impl Fn() + 'static) -> RwSignal<f64> {
    let content_height = RwSignal::new(0.0_f64);

    Effect::new(move |_| {
        read();
        if let Some(el) = measure_ref.get() {
            // Defer to the next frame so the twin reflects the new content.
            request_animation_frame(move || {
                let next = el.scroll_height() as f64;
                content_height.update(|h| {
                    if (*h - next).abs() > 2.0 {
                        *h = next;
                    }
                });
            });
        }
    });

    content_height
}
