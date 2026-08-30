//! The floating rail's mount point: a fixed wrapper that fades in over the
//! page from the window's left edge, plus the edge-hover affordance that
//! opens it. Self-gating — the page mounts it as a sibling of the reader
//! surface and it renders nothing while the shell controller says the
//! layout is docked.
//!
//! WHY A SIBLING, NOT A CHILD. The rail mounts OUTSIDE `.reader-bg`, which
//! is a stacking context (`position: relative` + `z-index: 0`): everything
//! inside it paints below the title bar's band, a SIBLING of `.reader-bg`
//! at `--z-bar`. A floating rail that the bar paints over loses its whole
//! header to the glass — the close, search and More buttons sit in the top
//! 48px, exactly where the band is. Out here the rail's own `--z-popover`
//! outranks the bar, so it covers the bar's left corner (the Library button
//! included) and takes the traffic lights with it, and the bar reads as one
//! full-width surface either way. The settings modal uses the same escape.
//!
//! THE RAIL FADES — it does not slide. The native traffic lights sit pinned
//! over this wrapper's top-left corner and can only appear and disappear,
//! so a transform slide off the edge would travel under buttons that cannot
//! follow it; a fade is the one motion the rail and the lights can make
//! together, and the shell controller's close hold times the fade out so
//! the lights release on the frame the rail finishes disappearing. The
//! class list is the contract the aside keeps too, so the two shapes live
//! here as literals rather than an interpolation — a `format!` per frame
//! invites a token to go missing. `fixed`, not `absolute`: this wrapper
//! sits OUTSIDE `.reader-bg`, so there is no positioned ancestor left to
//! resolve against and the viewport is the honest box.
//!
//! A faded-out rail is still a box on screen, so the wrapper drops to
//! `pointer-events-none` while it is closed — otherwise an invisible strip
//! down the window's left edge would swallow the very hover the edge strip
//! needs to reopen the rail.
//!
//! EDGE HOVER. Brushing the window's left edge (the 1.5px strip, shown only
//! while the rail is fully closed) or the rail itself holds it open; leaving
//! both lets it close after a short grace. The open restores the panel a
//! close last left behind — `ShellController::open_last_panel` — which is
//! the same tracker the close hold's paint hold uses, so there is exactly
//! one notion of "the last panel" in the app.

use std::time::Duration;

use leptos::children::ChildrenFn;
use leptos::prelude::*;

use crate::components::primitives::hooks::use_timeout::use_hover_visibility;
use crate::components::shell::controller::ShellController;

/// How long the pointer may be off the rail before it closes.
const HOVER_GRACE_MS: u64 = 250;

// `ChildrenFn`, not `Children`: `Show`'s children closure must be an `Fn`
// (it re-runs on every docked↔overlay flip), and only the `Rc`-backed
// children can be called from inside one.
#[component]
pub fn OverlayRail(shell: ShellController, children: ChildrenFn) -> impl IntoView {
    // Shown while the pointer is over the strip or the rail; a docked
    // layout postpones any hide forever, which parks the machine inert
    // (the edge effect below also refuses to act while docked).
    let hover = use_hover_visibility(
        Duration::from_millis(HOVER_GRACE_MS),
        move || !shell.is_overlay().get(),
    );

    // Edge-triggered open/close: only a transition of `visible` acts, and
    // only in overlay mode — the raw reads below are unconditional so the
    // effect keeps its subscriptions (see the components module rules).
    let prev_vis = StoredValue::new_local(false);
    Effect::new(move |_| {
        let vis = hover.visible.get();
        let was = prev_vis.get_value();
        prev_vis.set_value(vis);
        if !shell.is_overlay().get() {
            return;
        }
        if vis && !was && !shell.is_sidebar_open().get() {
            shell.open_last_panel();
        } else if !vis && was && shell.is_sidebar_open().get() {
            shell.close_sidebar();
        }
    });

    // The hover handles are `Rc`s, and `Show`'s children must stay `Fn` —
    // so the view's handlers bump Copy counter signals and these effects
    // (outside the view) relay them to the hover machine. Every bump is a
    // new value, so every enter/leave fires its side exactly once.
    let request_show = RwSignal::new(0u32);
    let request_hide = RwSignal::new(0u32);
    let show = hover.show.clone();
    Effect::new(move |_| {
        if request_show.get() > 0 {
            show();
        }
    });
    let hide = hover.hide_later.clone();
    Effect::new(move |_| {
        if request_hide.get() > 0 {
            hide();
        }
    });

    view! {
        // The edge strip: full-height, a hairline wide, and only while the
        // rail is fully closed (an open rail already owns these pixels).
        <Show when=move || shell.hover_strip_active().get()>
            <div
                class="fixed inset-y-0 left-0 z-[var(--z-bar)] w-1.5"
                on:mouseenter=move |_| request_show.update(|n| *n += 1)
            />
        </Show>
        <Show when=move || shell.is_overlay().get()>
            <div
                class=move || if shell.no_slide().get() { OVERLAY_STATIC } else { OVERLAY_FADES }
                class=("opacity-0", move || !shell.is_sidebar_open().get())
                class=("pointer-events-none", move || !shell.is_sidebar_open().get())
                on:mouseenter=move |_| request_show.update(|n| *n += 1)
                on:mouseleave=move |_| request_hide.update(|n| *n += 1)
            >
                {children()}
            </div>
        </Show>
    }
}

/// The floating rail's two shapes, as literals rather than an interpolation:
/// the class list is the contract the aside keeps too, and a `format!` per
/// frame invites a token to go missing. The fade's `duration-200` matches the
/// shell controller's `SIDEBAR_FADE_MS`, which is how the close hold lands
/// the native lights on the same frame the rail finishes disappearing.
const OVERLAY_STATIC: &str = "fixed inset-y-0 left-0 z-[var(--z-popover)] shadow-2xl";
const OVERLAY_FADES: &str = concat!(
    "fixed inset-y-0 left-0 z-[var(--z-popover)] shadow-2xl",
    " transition-opacity duration-200 ease-in-out"
);
