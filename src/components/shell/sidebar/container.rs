//! The rail's container: the `<aside>` and its four slots, written once and
//! mounted from one of two places — [`crate::components::shell::sidebar::push`]
//! (docked, a flex sibling of the page) or
//! [`crate::components::shell::sidebar::overlay`] (floating, a fixed
//! wrapper outside the reader's stacking context). The two mount points
//! cannot collapse into one component: an overlay rail that sat inside
//! `.reader-bg` would paint under the title bar's band, so it mounts as the
//! page's sibling instead — see `overlay.rs` for the full stacking story.
//! Each wrapper self-gates on the shell controller's layout, which is what
//! "routes" the rail between the two.
//!
//! The `<aside>` is ALWAYS mounted (in whichever slot is live). In the
//! DOCKED layout it slides its `width` between 18rem and 0 (single-phase, no
//! two-phase unmount); the inner content stays fixed at `w-72` so it never
//! collapses — `overflow-hidden` on the aside clips it while closed. In the
//! OVERLAY layout the aside keeps its full width for the rail's whole
//! life: the wrapper's opacity fade is the open/close there, and a width
//! collapse inside a fading box would read as a clip wipe, not a fade.
//! When collapsed (docked only) the content is made `inert` so the clipped
//! rail can't be tab-focused / activated.
//!
//! THE DOCKED TOGGLE SLIDES, unless told not to. The aside tweens its width
//! over `SIDEBAR_SLIDE_MS` (the shell controller's machine timing), and
//! `.sidebar-aside { contain: layout style }` keeps the reflow from
//! escaping the aside. The page follows the rail on every frame of that
//! slide — that is `follow_watcher`'s container follow, not a refit, and
//! the burst costs one raster pass because a follow holds its commit until
//! the container goes quiet.
//!
//! Tab switches (Thumbs ↔ Outline) still use `invisible` on the inactive
//! panel so the virtualization window stays engine-bound and a switch back
//! is instant. `hidden` (`display:none`) is still forbidden: its height
//! collapse would re-evict the window and re-render every thumb.
//!
//! Gotcha: each reactive `class=("name", cond)` toggle becomes one
//! `classList.add("name")` call — a space-separated token throws a swallowed
//! SyntaxError and the class is silently never applied. Keep every conditional
//! class to a SINGLE token (hence `w-0` and `border-r-0` as separate toggles).

use leptos::children::ViewFn;
use leptos::prelude::*;

use crate::state::SidebarMode;

/// Selector for the sliding aside itself. The toolbar's title measurement
/// observes this element (its width changes every frame of the slide, unlike
/// the row, which only changes inset at the end of the close hold) — the
/// selector lives next to the class it targets so they cannot drift apart.
pub(crate) const SIDEBAR_ASIDE_SELECTOR: &str = "aside.sidebar-aside";

/// Ask the visible panel to scroll to wherever the reader currently is.
///
/// A CustomEvent rather than a signal in `AppState`: this is a one-shot
/// gesture with no state to hold, and a counter signal would need to be read
/// by both panels and could not distinguish "asked twice" from "asked once"
/// without extra bookkeeping. Same mechanism the PDF link layer uses.
pub(crate) fn request_reveal_active() {
    crate::events::dispatch_event(crate::events::REVEAL_ACTIVE_EVENT);
}

#[component]
pub fn SidebarShell(
    mode: RwSignal<SidebarMode>,
    /// The floating layout: the aside keeps its full width and the overlay
    /// wrapper's fade is the open/close (see the module docs). Docked rails
    /// keep the width tween below.
    #[prop(into)]
    overlay: Signal<bool>,
    // `no_slide` freezes the width tween (Settings → Animations). Read TRACKED
    // here, unlike the machine effect that consults the same flag: the class
    // has to move in the frame the switch does.
    #[prop(into)] no_slide: Signal<bool>,
    #[prop(into)] header: ViewFn,
    #[prop(optional, into)] info_row: Option<ViewFn>,
    #[prop(into)] panels: ViewFn,
    #[prop(optional, into)] footer: Option<ViewFn>,
) -> impl IntoView {
    view! {
        <aside
            class="sidebar-aside flex h-full shrink-0 flex-col overflow-hidden border-r border-line bg-surface transition-[width] duration-300 ease-in-out"
            class=("w-72", move || {
                overlay.get() || matches!(mode.get(), SidebarMode::Thumbs | SidebarMode::Outline)
            })
            class=("w-0", move || !overlay.get() && mode.get() == SidebarMode::None)
            class=("border-r-0", move || !overlay.get() && mode.get() == SidebarMode::None)
            class=("no-slide", move || no_slide.get())
        >
            <div
                class="flex h-full w-72 min-h-0 flex-col"
                prop:inert=move || mode.get() == SidebarMode::None
            >
                {header.run()}
                {info_row.map(|row| row.run())}
                <div class="relative min-h-0 flex-1">
                    {panels.run()}
                </div>
                {footer.map(|row| row.run())}
            </div>
        </aside>
    }
}
