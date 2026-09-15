//! The modal sheet's chrome: one backdrop, one panel, one lane registration
//! and one Escape rule, in one place.
//!
//! Every sheet used to hand-roll the same four things: the dimmed backdrop
//! that closes on a click, the panel that stops the click from reaching it,
//! the overlay lane's arbitration and the Escape rule that peels one layer
//! at a time. Four copies of one contract is four places a sheet can quietly
//! differ about how it closes — and a sheet that closes differently from its
//! siblings is one the reader has to relearn.
//!
//! The sheet's own face is not here: header, body and footer are the
//! children, because those genuinely differ. The panel is a flex column with
//! a scrollable middle, the shape all of them already were.

use leptos::children::ChildrenFn;
use leptos::prelude::*;

use app_chrome::floating::dismiss::use_modal_escape;

use super::lanes::{OverlayPolicy, use_overlay_lane};

#[component]
pub fn ModalShell(
    /// Whether the sheet is up. The lane registry and the Escape rule both
    /// read this signal and the backdrop's click writes it — a sheet whose
    /// closing means more than "not open" (the conflict sheet's payload has
    /// to go with it) watches the signal in an effect of its own.
    open: RwSignal<bool>,
    /// What the dialog calls itself to a screen reader.
    aria_label: &'static str,
    /// The panel's width as a CSS value — `min(92vw, 420px)` — one sheet's
    /// width is its own fact and the chrome's job is to wear it.
    width: &'static str,
    /// The panel's height, for a sheet that sizes ITSELF rather than its
    /// content — the reader's settings modal, whose tabs share one box. `None`
    /// lets the body decide, under the shell's own `max-h-[86vh]`.
    #[prop(optional)]
    height: Option<&'static str>,
    /// `ChildrenFn`, not `Children`: `Show`'s children closure must be an
    /// `Fn`, and only children that can be called from inside one may ride
    /// it — the reason the sidebar's overlay rail gives for the same choice.
    children: ChildrenFn,
) -> impl IntoView {
    // One modal at a time, and a menu replaces it rather than stacking under
    // it — the same arbitration the reader's settings modal joins.
    use_overlay_lane(open, OverlayPolicy::MODAL);
    // A popover opened inside the sheet owns the press; the shared rule peels
    // one layer at a time.
    use_modal_escape(open);

    view! {
        <Show when=move || open.get()>
            <div
                class="fixed inset-0 z-[var(--z-popover)] flex items-center justify-center bg-black/45 p-4"
                on:click=move |_| open.set(false)
            >
                <div
                    class="flex max-h-[86vh] w-full flex-col overflow-hidden rounded-2xl border border-line bg-surface shadow-2xl"
                    style=format!(
                        "width:{width}{}",
                        height.map_or(String::new(), |h| format!(";height:{h}"))
                    )
                    // The panel is not the backdrop: a click inside the sheet
                    // is the sheet's.
                    on:click=move |ev| ev.stop_propagation()
                    role="dialog"
                    aria-label=aria_label
                >
                    {children()}
                </div>
            </div>
        </Show>
    }
}
