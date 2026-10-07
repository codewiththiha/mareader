//! Auto-hide surfaces: the whole hover machine, once.

use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;
use leptos::tachys::html::element::ElementType;
use wasm_bindgen::JsCast;

use crate::hooks::use_timeout::use_hover_visibility;

/// The grace period every chrome surface hides after.
pub const DEFAULT_HOVER_DELAY: Duration = Duration::from_millis(400);

/// How a surface reveals: grace period, hold, pin.
#[derive(Clone, Copy)]
pub struct HoverConfig {
    pub delay: Duration,
    /// While true the surface never hides (an open popover, a drag).
    pub hold: Option<Signal<bool>>,
    /// While true the surface is always visible, hover or not.
    pub pin: Option<Signal<bool>>,
}

impl Default for HoverConfig {
    fn default() -> Self {
        Self {
            delay: DEFAULT_HOVER_DELAY,
            hold: None,
            pin: None,
        }
    }
}

/// One pointer edge, cloned out to every element that binds it.
type Handler = Rc<dyn Fn()>;

/// A reveal controller: visibility plus the two pointer edges.
#[derive(Clone)]
pub struct HoverReveal {
    /// What to render from: `pin || hovered_visible`. Writes stay the hook's.
    pub visible: Signal<bool>,
    enter: Handler,
    leave: Handler,
}

impl HoverReveal {
    pub fn enter(&self) {
        (self.enter)();
    }

    pub fn leave(&self) {
        (self.leave)();
    }

    /// A fresh `(enter, leave)` pair for one element.
    pub fn bind(&self) -> (Handler, Handler) {
        (Rc::clone(&self.enter), Rc::clone(&self.leave))
    }
}

/// Build a reveal controller owned by the current reactive owner.
pub fn use_hover_reveal(config: HoverConfig) -> HoverReveal {
    let hold = config.hold;
    reveal(
        config.delay,
        move || hold.is_some_and(|h| h.get()),
        config.pin,
    )
}

/// Sugar for the common shape: a closure hold, no pin.
pub fn use_hover_reveal_with(
    delay: Duration,
    hold: impl Fn() -> bool + Copy + 'static,
) -> HoverReveal {
    reveal(delay, hold, None)
}

/// The one implementation both entry points funnel into.
fn reveal(
    delay: Duration,
    held: impl Fn() -> bool + Copy + 'static,
    pin: Option<Signal<bool>>,
) -> HoverReveal {
    let hover = use_hover_visibility(delay, held);

    // `StoredValue`: the recheck effect must not track this flag.
    let hovered = StoredValue::new_local(false);
    let enter: Rc<dyn Fn()> = Rc::new({
        let show = hover.show.clone();
        move || {
            hovered.set_value(true);
            show();
        }
    });
    let leave: Rc<dyn Fn()> = Rc::new({
        let hide = hover.hide_later.clone();
        move || {
            hovered.set_value(false);
            hide();
        }
    });

    // A hold released while the pointer is away produces no
    // `mouseleave`; this settles it.
    let recheck = hover.hide_later.clone();
    let shown = hover.visible;
    Effect::new(move |_| {
        if !held() && !hovered.get_value() && shown.get_untracked() {
            recheck(); // the hold is gone and so is the pointer → hide
        }
    });

    let hovered_visible: Signal<bool> = hover.visible.into();
    let visible = match pin {
        Some(pin) => Signal::derive(move || pin.get() || hovered_visible.get()),
        None => hovered_visible,
    };

    HoverReveal {
        visible,
        enter,
        leave,
    }
}

/// Whether the point still lands on the surface, after capture.
fn released_on(surface: &web_sys::Element, x: f32, y: f32) -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.element_from_point(x, y))
        .is_some_and(|el| surface.contains(Some(&el)))
}

/// The pointer-capture half, opt-in: the handler to bind alongside
/// `on:pointerdown`. `reveal` is taken by value.
pub fn use_drag_hold<E>(
    surface: NodeRef<E>,
    dragging: RwSignal<bool>,
    reveal: HoverReveal,
) -> impl Fn(leptos::ev::PointerEvent) + Clone + 'static
where
    E: ElementType + 'static,
    E::Output: JsCast + Clone + AsRef<web_sys::Element> + 'static,
{
    let (_, leave) = reveal.bind();
    move |ev: leptos::ev::PointerEvent| {
        if !dragging.get_untracked() {
            return;
        }
        dragging.set(false);
        // Capture swallows `mouseleave`; learn the real position here.
        let over = surface
            .get()
            .is_some_and(|el| released_on(el.as_ref(), ev.client_x() as f32, ev.client_y() as f32));
        if !over {
            leave();
        }
    }
}
