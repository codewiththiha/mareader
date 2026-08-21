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
    /// Used when `anchor` is hidden (zero-size), e.g. a collapsed toolbar
    /// control re-anchored at the overflow "…" button.
    #[prop(optional)]
    fallback_anchor: NodeRef<html::Div>,
    /// Desired panel width in CSS px (custom per menu).
    #[prop(default = 256)]
    width: u32,
    /// Min distance from viewport edges.
    #[prop(default = 8)]
    margin: u32,
    /// Extra classes (padding, max-h, overflow…).
    #[prop(optional, into)]
    class: String,
    /// Whether opening this popover holds the reader titlebar open.
    /// Defaults to true; sidebar popovers (MoreMenu) set this to false.
    #[prop(default = true)]
    hold_titlebar: bool,
    children: ChildrenFn,
) -> impl IntoView {
    let panel_ref: NodeRef<html::Div> = NodeRef::new();
    let style_sig = RwSignal::new(String::new());

    // Window-aware placement: right-aligned to the trigger, clamped into the
    // viewport, flipped ABOVE the trigger when there is no room below.
    let place = move || {
        let primary = anchor.get();
        let fallback = fallback_anchor.get();
        let a = match (primary, fallback) {
            (Some(p), Some(f)) if p.get_bounding_client_rect().width() < 1.0 => f,
            (Some(p), _) => p,
            (None, Some(f)) => f,
            (None, None) => return,
        };
        let Some(win) = web_sys::window() else { return };
        let ar = a.get_bounding_client_rect();
        let pw = width as f64;
        let ph = panel_ref
            .get()
            .map(|p| p.get_bounding_client_rect().height())
            .unwrap_or(200.0);
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
        let mut left = (ar.right() - pw).clamp(m, (win_w - pw - m).max(m));
        let (top_raw, origin) = if ar.bottom() + ph + m <= win_h {
            (ar.bottom() + 4.0, "top right") // opens downward
        } else {
            ((ar.top() - ph - 4.0).max(m), "bottom right") // opens upward
        };
        let mut top = top_raw;

        // ── FIX ──────────────────────────────────────────────────────────
        // WebKit treats `backdrop-filter` as a containing block for
        // `position: fixed` descendants.  When the anchor sits inside
        // #toolbar-row (.toolbar-glass), the popover's fixed origin is the
        // toolbar row, NOT the viewport.  Subtract the row's viewport offset
        // so the coordinates become row-relative.
        if let Some(doc) = win.document() {
            if let Some(row) = doc.get_element_by_id("toolbar-row") {
                let anchor_in_row = row.contains(Some(&a));
                if anchor_in_row {
                    let rr = row.get_bounding_client_rect();
                    left -= rr.left();
                    top  -= rr.top();
                }
            }
        }
        // ─────────────────────────────────────────────────────────────────

        style_sig.set(format!(
            "left:{left:.1}px;top:{top:.1}px;width:{pw:.0}px;transform-origin:{origin}"
        ));
    };

    // Place once the panel mounts; re-clamp on window resize while open.
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        place();
        request_animation_frame(place);
        let h = window_event_listener_untyped("resize", move |_| place());
        on_cleanup(move || h.remove());
    });

    // Keep the reader titlebar from auto-hiding while this menu is up.
    let held_ctx = use_context::<TitleBarCtx>();
    if hold_titlebar && let Some(ctx) = held_ctx {
        let was_holding = StoredValue::new_local(false);
        Effect::new(move |_| {
            let is_open = open.get();
            let holding = was_holding.get_value();
            if is_open && !holding {
                ctx.held_count.update(|c| *c += 1);
                was_holding.set_value(true);
            } else if !is_open && holding {
                ctx.held_count.update(|c| *c = c.saturating_sub(1));
                was_holding.set_value(false);
            }
        });
        on_cleanup(move || {
            if was_holding.get_value() {
                ctx.held_count.update(|c| *c = c.saturating_sub(1));
                was_holding.set_value(false);
            }
        });
    }

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
                let in_fallback = fallback_anchor
                    .get()
                    .map(|f| f.contains(Some(&target)))
                    .unwrap_or(false);
                let in_panel = panel_ref
                    .get()
                    .map(|p| p.contains(Some(&target)))
                    .unwrap_or(false);
                if !in_anchor && !in_fallback && !in_panel {
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
