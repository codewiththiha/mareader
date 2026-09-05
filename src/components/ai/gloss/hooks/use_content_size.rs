//! Measure the real content height of an invisible "twin" node so the gloss
//! card can size itself to its content — no minimum: a two-line answer gets
//! a two-line card, and content growing the twin later animates the card
//! open a little further. The twin must be a pixel-exact replica of the
//! card's scroll column.
//!
//! Feature-shaped measuring, so this hook lives with the gloss hooks rather
//! than in `app-chrome` (the reusable chrome crate); the generic
//! `ResizeObserver` it rides on stays there.
//!
//! Two triggers keep the measurement honest so the card can NEVER under-size
//! itself (which is what shows up as a scroll cursor the user has to reopen
//! to clear):
//! * a reactive one — when a tracked signal changes (a streamed snapshot
//!   patches content in, the word wraps differently), the effect re-reads the
//!   twin's `scroll_height`;
//! * a layout one — a `ResizeObserver` on the twin fires whenever the twin's
//!   own box actually changes height (a font swap, a section appearing, the
//!   answer streaming in), independent of whether a tracked signal changed.
//!   This is what covers the very first appearance, where the twin is read
//!   before content is present and the deferred reactive read is missed.

use leptos::html;
use leptos::prelude::*;

use app_chrome::hooks::use_resize_observer::use_resize_observer;

/// How much the twin's height must move before it is worth pushing to the
/// card. A 1px change is noise (the browser sub-pixel rounding); re-flowing
/// the card's spring for less than 2px would just throb.
const JITTER_PX: f64 = 2.0;

/// The height the card should adopt, or `None` when the move is beneath the
/// jitter gate. Kept pure (no DOM, no signal) so the guard that swallowed the
/// stale re-read in the shimmer race is unit-testable on the host.
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

/// Measure now AND on the next frame.
///
/// The synchronous read is the correctness half: when a content signal flips
/// (a streamed snapshot lands), the twin is patched in place, and the
/// `ResizeObserver` reaction for that same change can run in the frame before
/// this effect's rAF. If the observer writes the new height first, a rAF-only
/// read then sees an unchanged number and the jitter gate swallows the write —
/// leaving `content_height` at the stale (shimmer) height so the card never
/// grows to fit the streamed text. Reading `scroll_height` synchronously here
/// cannot lose that ordering, because it happens in the same flush as the
/// content write. The extra rAF is the settling half: it catches font/scrollbar
/// layout that lands one frame later.
fn measure(el: &web_sys::HtmlDivElement, content_height: RwSignal<f64>) {
    let el = el.clone();
    write_height(&el, content_height);
    request_animation_frame(move || write_height(&el, content_height));
}

/// Returns the live height signal fed by the measure twin.
///
/// `read` is invoked inside the tracking effect, so callers express which
/// signals should trigger a re-measure by simply reading them there (e.g.
/// `move || { let _ = info.get(); let _ = word.get(); }`).
pub fn use_content_size(measure_ref: NodeRef<html::Div>, read: impl Fn() + 'static) -> RwSignal<f64> {
    let content_height = RwSignal::new(0.0_f64);

    // Reactive trigger: any tracked signal that drives the twin's content.
    Effect::new(move |_| {
        read();
        if let Some(el) = measure_ref.get() {
            measure(&el, content_height);
        }
    });

    // Layout backstop: whatever changes the twin's height — a streamed
    // snapshot, a font swap, the width returning from a clamp — opens/keeps
    // the card at its real size. Deleting this is why the card used to need
    // a close-and-reopen to grow on first appearance.
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
        // A 1px settle is the browser's sub-pixel rounding, not a real change:
        // it must not re-flow the card's spring.
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
