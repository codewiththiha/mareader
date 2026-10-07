//! Measure an invisible twin node so the card sizes to its content.

use leptos::html;
use leptos::prelude::*;

use app_chrome::hooks::use_resize_observer::use_resize_observer;

/// How much the twin's height must move to be worth a re-flow.
const JITTER_PX: f64 = 2.0;

/// The height to adopt, or `None` beneath the jitter gate.
fn accepted_height(current: f64, next: f64) -> Option<f64> {
    ((current - next).abs() > JITTER_PX).then_some(next)
}

/// A jitter-guarded write of the twin's current height to the signal.
fn write_height(el: &web_sys::HtmlDivElement, content_height: RwSignal<f64>) {
    let next = el.scroll_height() as f64;
    content_height.update(|h| {
        if let Some(next) = accepted_height(*h, next) {
            *h = next;
        }
    });
}

/// Measure now and on the next frame.
fn measure(el: &web_sys::HtmlDivElement, content_height: RwSignal<f64>) {
    let el = el.clone();
    write_height(&el, content_height);
    request_animation_frame(move || write_height(&el, content_height));
}

/// The live height signal fed by the measure twin.
pub fn use_content_size(
    measure_ref: NodeRef<html::Div>,
    read: impl Fn() + 'static,
) -> RwSignal<f64> {
    let content_height = RwSignal::new(0.0_f64);

    // Reactive trigger: any tracked signal that drives the twin's content.
    Effect::new(move |_| {
        read();
        if let Some(el) = measure_ref.get() {
            measure(&el, content_height);
        }
    });

    // Layout backstop: whatever changes the twin's height.
    let observer_ref = measure_ref;
    use_resize_observer(observer_ref, move |_| {
        if let Some(el) = measure_ref.get() {
            measure(&el, content_height);
        }
    });

    content_height
}

#[cfg(test)]
mod tests {
    use super::accepted_height;

    #[test]
    fn the_jitter_gate_drops_sub_2px_noise() {
        // A 1px settle is sub-pixel rounding, not a real change.
        assert_eq!(accepted_height(400.0, 401.0), None);
        assert_eq!(accepted_height(400.0, 399.0), None);
    }

    #[test]
    fn a_real_height_change_is_accepted() {
        assert_eq!(accepted_height(150.0, 400.0), Some(400.0));
        assert_eq!(accepted_height(400.0, 150.0), Some(150.0));
        // The gate is strict: a move of exactly the gate width is still noise.
        assert_eq!(accepted_height(400.0, 402.0), None);
        assert_eq!(accepted_height(400.0, 402.1), Some(402.1));
    }
}
