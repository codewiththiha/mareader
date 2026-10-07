//! A morphing floating surface driven by a box signal.

use leptos::children::Children;
use leptos::prelude::*;

use app_chrome::floating::types::FloatBox;

/// A morphing floating surface driven by a box signal.
#[component]
pub fn FloatingCard(
    /// The current box (x/y/w/h/r) — written per frame by a spring.
    box_: Signal<FloatBox>,
    /// The box the CONTENT wrapper is sized to, so text never reflows.
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
    /// Drag-handle slot, above the scroll area.
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
    /// Hides the inner scroller's scrollbar, which would otherwise
    /// narrow the content column.
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

    // Static for the component's lifetime.
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
                    class="flex h-full min-h-0 flex-col overflow-y-auto overscroll-contain"
                    data-gloss-scroll=hide_scrollbar.then_some("")
                >
                    {children()}
                </div>
            </div>
        </div>
    }
}
