//! The app's anchored MENU: the [`Popover`] plus shared menu policy.

use leptos::children::ChildrenFn;
use leptos::html;
use leptos::prelude::*;

use crate::components::primitives::floating::popover::Popover;
use crate::components::primitives::overlay::lanes::{OverlayPolicy, use_overlay_lane};
use app_chrome::floating::types::PlacementSide;
use app_chrome::titlebar::root::TitleBarCtx;

#[component]
pub fn MenuPopover(
    open: RwSignal<bool>,
    anchor: NodeRef<html::Div>,
    /// The panel's width in CSS px.
    #[prop(into, default = Signal::stored(256u32))]
    width: Signal<u32>,
    #[prop(default = 8)] margin: u32,
    #[prop(optional, into)] class: String,
    /// Whether opening this popover holds the reader titlebar open.
    #[prop(default = true)]
    hold_titlebar: bool,
    #[prop(default = PlacementSide::Auto)] placement: PlacementSide,
    /// Id of an element whose viewport offset is subtracted.
    #[prop(optional)]
    coordinate_space: Option<&'static str>,
    /// Which surfaces this menu may coexist with.
    #[prop(default = OverlayPolicy::MENU)]
    policy: OverlayPolicy,
    children: ChildrenFn,
) -> impl IntoView {
    // Registration only: arbitration reacts to the SIGNAL.
    use_overlay_lane(open, policy);
    // Hold/release the titlebar as the popover opens and closes.
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
