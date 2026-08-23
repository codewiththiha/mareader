//! The application's title bar: wires the generic [`TitleBar`] shell to the
//! app (pin persistence, native traffic lights, sidebar inset held through
//! the close slide, search hold).

use leptos::children::ViewFn;
use leptos::prelude::*;

use crate::components::chrome::title_bar::{TitleBar, TitleBarCtx};
use crate::components::panels::sidebar_shell::SidebarChromeCtx;
use crate::state::AppState;
use crate::state::SidebarMode;
use crate::storage::save_settings;

#[component]
pub fn AppTitleBar(
    state: AppState,
    #[prop(into)] left: ViewFn,
    #[prop(into)] right: ViewFn,
    /// Native traffic lights. The generic TitleBar does not know about them;
    /// this wrapper decides whether to mount the effect.
    #[prop(default = true)]
    show_traffic_lights: bool,
    children: Children,
) -> impl IntoView {
    let pinned = RwSignal::new(state.settings.with(|s| s.titlebar_pinned));
    let on_pin_change = Callback::new(move |p: bool| {
        state.settings.update(|s| s.titlebar_pinned = p);
        if let Err(e) = save_settings(&state.settings.with(|s| s.clone())) {
            e.report();
        }
    });
    // The open floating search holds the bar (like an open popover).
    let extra_hold = Signal::derive(move || state.reader.search.visible.get());
    // The sidebar owns the left inset while it is painted, not merely while
    // its mode is open. The mode flips to `None` on the close click, but the
    // aside keeps sliding for 300ms; the reader page publishes that window as
    // `present`. Pages without a sidebar fall back to the raw mode.
    let chrome = use_context::<SidebarChromeCtx>();
    let band_inset = Signal::derive(move || match chrome {
        Some(chrome) => chrome.present.get(),
        None => state.ui.sidebar.get() != SidebarMode::None,
    });

    view! {
        <TitleBar
            pinned=pinned
            on_pin_change=on_pin_change
            extra_hold=extra_hold
            band_inset=band_inset
            left=left
            right=right
        >
            {show_traffic_lights.then(|| view! { <TrafficLights present=band_inset /> })}
            {children()}
        </TitleBar>
    }
}

/// Native traffic lights: on while pinned/hovered, or while the sidebar is
/// painted. Mounted inside the [`TitleBar`] children so it can read the shared
/// [`TitleBarCtx`].
#[component]
fn TrafficLights(present: Signal<bool>) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    // `setHidden:` is binary. Holding this through the close slide makes the
    // lights leave with the rail rather than flickering on hover while it moves.
    let last_sent = StoredValue::new_local(None::<bool>);
    Effect::new(move |_| {
        let Some(ctx) = ctx else {
            return;
        };
        let on = ctx.visible.get() || present.get();
        // Both terms can update in one reactive flush; avoid redundant native
        // calls so the window only sees actual visibility transitions.
        if last_sent.get_value() == Some(on) {
            return;
        }
        last_sent.set_value(Some(on));
        wasm_bindgen_futures::spawn_local(async move {
            pdf_engine::api::set_traffic_lights(on).await;
        });
    });
}
