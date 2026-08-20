//! Reusable window-aware menu container. `position: fixed` escapes the
//! sidebar's `overflow-hidden`, the panel is clamped into the viewport, and it
//! flips upward when there is no room below the trigger ("open out" animation,
//! window aware). Width is a prop so each menu can size itself.
//!
//! Outside-click + Escape dismissal and the "keep the titlebar open" hold are
//! owned here, so every menu gets them for free.

use leptos::children::ChildrenFn;
use leptos::html;
use leptos::prelude::*;

use super::titlebar_provider::TitleBarCtx;

#[component]
pub fn Popover(
    open: RwSignal<bool>,
    /// NodeRef of the trigger wrapper the panel anchors to.
    anchor: NodeRef<html::Div>,
    /// Desired panel width in CSS px (custom per menu).
    #[prop(default = 256)]
    width: u32,
    /// Min distance from viewport edges.
    #[prop(default = 8)]
    margin: u32,
    /// Extra classes (padding, max-h, overflow…).
    #[prop(optional, into)]
    class: String,
    children: ChildrenFn,
) -> impl IntoView {
    let panel_ref: NodeRef<html::Div> = NodeRef::new();
    let style_sig = RwSignal::new(String::new());

    // Window-aware placement: right-aligned to the trigger, clamped into the
    // viewport, flipped ABOVE the trigger when there is no room below.
    let place = move || {
        let Some(a) = anchor.get() else { return };
        let Some(panel) = panel_ref.get() else { return };
        let Some(win) = web_sys::window() else { return };
        let ar = a.get_bounding_client_rect();
        let pw = width as f64;
        let ph = panel.get_bounding_client_rect().height();
        let win_w = win
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(1280.0);
        let win_h = win
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(800.0);
        let m = margin as f64;
        let left = (ar.right() - pw).clamp(m, (win_w - pw - m).max(m));
        let (top, origin) = if ar.bottom() + ph + m <= win_h {
            (ar.bottom() + 4.0, "top right") // opens downward
        } else {
            ((ar.top() - ph - 4.0).max(m), "bottom right") // opens upward
        };
        style_sig.set(format!(
            "left:{left:.1}px;top:{top:.1}px;width:{pw:.0}px;transform-origin:{origin}"
        ));
    };

    // Place once the panel mounts; re-clamp on window resize while open.
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        request_animation_frame(place);
        let h = window_event_listener_untyped("resize", move |_| place());
        on_cleanup(move || h.remove());
    });

    // Keep the reader titlebar from auto-hiding while this menu is up.
    let held_ctx = use_context::<TitleBarCtx>();
    Effect::new(move |_| {
        if let Some(ctx) = held_ctx {
            ctx.held.set(open.get());
        }
    });

    // Outside-click + Escape dismissal owned HERE so every menu gets it free.
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let pd = window_event_listener(
            leptos::ev::pointerdown,
            move |ev: leptos::ev::PointerEvent| {
                let target: web_sys::Node = event_target(&ev);
                let in_anchor = anchor
                    .get()
                    .map(|a| a.contains(Some(&target)))
                    .unwrap_or(false);
                let in_panel = panel_ref
                    .get()
                    .map(|p| p.contains(Some(&target)))
                    .unwrap_or(false);
                if !in_anchor && !in_panel {
                    open.set(false);
                }
            },
        );
        let kd = window_event_listener(
            leptos::ev::keydown,
            move |ev: leptos::ev::KeyboardEvent| {
                if ev.key() == "Escape" {
                    open.set(false);
                }
            },
        );
        on_cleanup(move || {
            pd.remove();
            kd.remove();
        });
    });

    // Static, but parked in a signal so the Show children closure (an `Fn`)
    // can read it without moving a non-Copy `String`.
    let panel_class = if class.is_empty() {
        "menu-popover fixed z-50 rounded-lg border border-line bg-surface shadow-lg"
            .to_string()
    } else {
        format!(
            "menu-popover fixed z-50 rounded-lg border border-line bg-surface shadow-lg {class}"
        )
    };
    let class_sig = RwSignal::new(panel_class);

    view! {
        <Show when=move || open.get()>
            <div
                node_ref=panel_ref
                class=move || class_sig.get()
                style=move || style_sig.get()
            >
                {children()}
            </div>
        </Show>
    }
}
