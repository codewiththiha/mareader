//! App-shell wrapper around the floating [`Popover`] primitive. The
//! primitive reports open-state transitions through `on_open_change` and
//! knows nothing about the chrome layer; this wrapper owns the two pieces of
//! shell policy every anchored menu shares:
//!
//! * **holding the reader titlebar open while the menu is up**, so the bar
//!   does not auto-hide under your hand mid-click (the sidebar's More menu
//!   opts out — it does not sit under the bar);
//! * **lane arbitration** (`OverlayPolicy::MENU`): one menu at a time, and a
//!   menu replaces an open modal instead of stacking under it. Registering it
//!   HERE is what makes that automatic for every menu, so a new one cannot
//!   forget it.
//!
//! Not a second popover primitive: it is a thin, single-purpose composition —
//! holds, policy, pass-through. New policy belongs in its own wrapper, not here.

use leptos::children::ChildrenFn;
use leptos::html;
use leptos::prelude::*;

use app_chrome::titlebar::root::TitleBarCtx;
use crate::components::primitives::floating::popover::Popover;
use app_chrome::floating::types::PlacementSide;
use crate::components::primitives::overlay::lanes::{OverlayPolicy, use_overlay_lane};

#[component]
pub fn MenuPopover(
    open: RwSignal<bool>,
    anchor: NodeRef<html::Div>,
    #[prop(default = 256)]
    width: u32,
    #[prop(default = 8)]
    margin: u32,
    #[prop(optional, into)]
    class: String,
    /// Whether opening this popover holds the reader titlebar open.
    /// Defaults to true; anchored popovers that sit clear of the bar (for
    /// example the settings rows' dropdowns) set this to false.
    #[prop(default = true)]
    hold_titlebar: bool,
    #[prop(default = PlacementSide::Auto)]
    placement: PlacementSide,
    /// Id of an element whose viewport offset must be subtracted (WebKit's
    /// `backdrop-filter` containing block) — pass `"toolbar-row"` when the
    /// anchor sits inside the glass toolbar row.
    #[prop(optional)]
    coordinate_space: Option<&'static str>,
    /// Which surfaces this menu may coexist with. The default
    /// ([`OverlayPolicy::MENU`]) takes part in the app's menu/modal
    /// arbitration; a surface that genuinely wants to float alongside
    /// everything else passes a policy with both `occupies` and `displaces`
    /// set to `Lanes::default()`.
    #[prop(default = OverlayPolicy::MENU)]
    policy: OverlayPolicy,
    children: ChildrenFn,
) -> impl IntoView {
    // Registration only: arbitration reacts to the SIGNAL, and every path that
    // can close this menu (the trigger, Escape, an outside press) writes it.
    use_overlay_lane(open, policy);
    // Hold/release the titlebar as the popover opens/closes. The primitive
    // only reports transitions; the counts stay here, at the shell.
    let on_open_change = if hold_titlebar {
        use_context::<TitleBarCtx>().map(|ctx| {
            Callback::new(move |open: bool| {
                if open {
                    ctx.held_count.update(|c| *c += 1);
                } else {
                    ctx.held_count.update(|c| *c = c.saturating_sub(1));
                }
            })
        })
    } else {
        None
    };

    view! {
        <Popover
            open=open
            anchor=anchor
            width=width
            margin=margin
            class=class
            placement=placement
            coordinate_space=coordinate_space
            on_open_change=on_open_change
        >
            {children()}
        </Popover>
    }
}
