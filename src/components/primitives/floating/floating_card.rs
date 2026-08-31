//! FloatingCard — the advanced floating-surface primitive that AI-style
//! cards compose instead of being forced into a simple menu popover.
//!
//! One fixed box whose `left/top/width/height/border-radius` come from a
//! [`FloatBox`] (typically sprung by [`crate::components::primitives::motion::spring::use_spring_box`]),
//! with an inner content wrapper sized to the *expanded* target so text never
//! reflows as the box morphs, an optional drag-handle slot, progress-driven
//! opacity/pointer-events, and an optional scroll area.
//!
//! Phase styling (fills, shadows, data-phase attributes) is the caller's
//! policy: `surface_style` / `data_phase` / `role` / `aria_label` pass it
//! through.

use leptos::children::Children;
use leptos::prelude::*;

use super::types::FloatBox;

/// A morphing floating surface driven by a box signal.
#[component]
pub fn FloatingCard(
    /// The current box (x/y/w/h/r) — written per frame by a spring.
    box_: Signal<FloatBox>,
    /// The box the CONTENT wrapper is sized to. Usually the final expanded
    /// target, so text never reflows while the box morphs.
    expanded: Signal<FloatBox>,
    /// Extra inline style appended after the box geometry (fills, shadows…).
    #[prop(optional)]
    surface_style: Option<Signal<String>>,
    /// Extra classes on the surface.
    #[prop(optional, into)]
    class: Option<String>,
    /// Content opacity (0..1), progress/phase driven by the caller.
    content_opacity: Signal<f64>,
    /// Whether the content is interactive (pointer-events) right now.
    content_interactive: Signal<bool>,
    /// Drag handle slot — rendered above the scroll area, faded in by
    /// progress by the caller's opacity signal if desired.
    #[prop(optional)]
    drag_handle: Option<Children>,
    /// Phase marker for CSS/state selectors (`data-phase="processing"`).
    #[prop(optional)]
    data_phase: Option<Signal<&'static str>>,
    /// ARIA role while expanded (e.g. `"dialog"`).
    #[prop(optional)]
    role: Option<Signal<&'static str>>,
    /// ARIA label while expanded (e.g. "Gloss for x").
    #[prop(optional)]
    aria_label: Option<Signal<String>>,
    /// Extra classes for the scroll area (padding etc.).
    #[prop(optional)]
    scroll_class: Option<&'static str>,
    /// Hide the inner scroller's native scrollbar. Floating surfaces must
    /// not show a gutter — and a layout-consuming scrollbar would narrow
    /// the content column, making the real content taller than the
    /// scrollbar-less measure twin, leaving the card permanently short.
    #[prop(default = false)]
    hide_scrollbar: bool,
    children: Children,
) -> impl IntoView {
    let surface_style_extra = surface_style;
    let surface_style = Signal::derive(move || {
        let b = box_.get();
        let mut s = format!(
            "position:fixed;left:{}px;top:{}px;width:{}px;height:{}px;border-radius:{}px;",
            b.x, b.y, b.w, b.h, b.r
        );
        if let Some(extra) = &surface_style_extra {
            s.push_str(&extra.get());
        }
        s
    });

    let content_style = Signal::derive(move || {
        let e = expanded.get();
        let opacity = content_opacity.get();
        let interactive = content_interactive.get();
        format!(
            "width:{}px;height:{}px;opacity:{};pointer-events:{};",
            e.w,
            e.h,
            opacity,
            if interactive { "auto" } else { "none" }
        )
    });

    let scroll_class = scroll_class.unwrap_or("");
    // Static for the component's lifetime: handed to the view as a plain
    // attribute (no reactive closure, no signal) so nothing suggests it
    // ever changes.
    let surface_class = class.unwrap_or_default();

    view! {
        <div
            class=surface_class
            data-phase=move || data_phase.map(|p| p.get()).unwrap_or("")
            role=move || role.map(|r| r.get()).unwrap_or("")
            aria-label=move || aria_label.map(|l| l.get()).unwrap_or_default()
            style=move || surface_style.get()
        >
            // Content wrapper: sized to the expanded target, faded in by the
            // caller's progress/phase signal.
            <div
                class="absolute left-0 top-0 overflow-hidden text-ink"
                style=move || content_style.get()
            >
                {drag_handle.map(|h| h())}
                <div
                    class=format!("flex h-full min-h-0 flex-col overflow-y-auto overscroll-contain {scroll_class}")
                    data-gloss-scroll=hide_scrollbar.then_some("")
                >
                    {children()}
                </div>
            </div>
        </div>
    }
}
