//! App-shell wrapper around the floating [`Popover`] primitive. The
//! primitive reports open-state transitions through `on_open_change` and
//! knows nothing about the chrome layer; this wrapper owns the one piece of
//! shell policy every anchored menu shares (except the sidebar's More menu,
//! which opts out): **holding the reader titlebar open while the menu is
//! up**, so the bar does not auto-hide under your hand mid-click.
//!
//! Not a `SuperPopover`: it is a thin, single-purpose composition — hold
//! bookkeeping + pass-through. New policy belongs in its own wrapper, not
//! here.

use leptos::children::ChildrenFn;
use leptos::html;
use leptos::prelude::*;

use crate::components::app_shell::title_bar::TitleBarCtx;
use crate::components::primitives::floating::popover::Popover;
use crate::components::primitives::floating::types::PlacementSide;

#[component]
pub fn MenuPopover(
    open: RwSignal<bool>,
    anchor: NodeRef<html::Div>,
    #[prop(optional)]
    fallback_anchor: NodeRef<html::Div>,
    #[prop(default = 256)]
    width: u32,
    #[prop(default = 8)]
    margin: u32,
    #[prop(optional, into)]
    class: String,
    /// Whether opening this popover holds the reader titlebar open.
    /// Defaults to true; sidebar popovers (MoreMenu) set this to false.
    #[prop(default = true)]
    hold_titlebar: bool,
    #[prop(default = PlacementSide::Auto)]
    placement: PlacementSide,
    /// Id of an element whose viewport offset must be subtracted (WebKit's
    /// `backdrop-filter` containing block) — pass `"toolbar-row"` when the
    /// anchor sits inside the glass toolbar row.
    #[prop(optional)]
    coordinate_space: Option<&'static str>,
    children: ChildrenFn,
) -> impl IntoView {
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
            fallback_anchor=fallback_anchor
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
