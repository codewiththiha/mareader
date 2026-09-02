//! The application's title bar: wires the generic [`TitleBar`] shell to the
//! app. The layout facts it feeds the shell — the band's inset, the row's
//! traffic-light gutter, the lights' two hosting signals — are the shell
//! controller's answers, and the pin state IS the controller's (created
//! from settings, persisted through it), so this wrapper only adapts:
//! context → props for a chrome crate that must stay application-agnostic.

use leptos::children::ViewFn;
use leptos::prelude::*;

use app_chrome::platform::{is_macos, uses_frameless_controls};
use app_chrome::titlebar::root::TitleBar;
use app_chrome::window::traffic_lights::TrafficLights;
use app_chrome::window::WindowControls;

use crate::components::shell::controller::ShellController;
use crate::state::AppState;

#[component]
pub fn AppTitleBar(
    state: AppState,
    #[prop(into)] left: ViewFn,
    /// Centered overlay passed through to the generic title bar. Defaults to empty.
    #[prop(into, default = ViewFn::from(|| ()))] center: ViewFn,
    #[prop(into)] right: ViewFn,
    children: Children,
) -> impl IntoView {
    // The page (reader or library) provides the shell controller; the
    // library's is rail-less, which is how its bar keeps the full window
    // width, its gutter and its lights.
    let shell = use_context::<ShellController>()
        .expect("the page provides the shell controller");
    let pinned = shell.titlebar_pinned;
    let on_pin_change = Callback::new(move |p: bool| shell.set_titlebar_pinned(p));
    // The open floating search holds the bar (like an open popover).
    let extra_hold = Signal::derive(move || state.reader.search.visible.get());

    // Window chrome is platform-split, and the split is fixed for the
    // process: macOS drives its native traffic lights (shown/hidden and
    // guttered by the shell controller), while Windows and Linux run
    // frameless — the platform config strips their title bar — and get
    // the app's own caption cluster at the bar's far edge. Probed once
    // (app_chrome::platform), so neither branch is reactive.
    let macos = is_macos();
    let frameless = uses_frameless_controls();
    // The native lights' two hosts, as the controller computes them: the
    // rail's header gutter while the rail is painted (independent of the
    // bar), the bar's own gutter while the bar owes the lights one. The
    // lights component is chrome, so it receives the answers instead of
    // asking the controller itself.
    let rail_hosted = shell.rail_present();
    let bar_hosted = shell.bar_gutter();

    view! {
        <TitleBar
            pinned=pinned
            on_pin_change=on_pin_change
            extra_hold=extra_hold
            band_inset=shell.band_inset()
            left_gutter=shell.titlebar_left_gutter()
            left=left
            center=center
            right=right
            end=ViewFn::from(move || {
                if frameless {
                    view! { <WindowControls maximized=state.ui.window_maximized /> }.into_any()
                } else {
                    ().into_any()
                }
            })
        >
            // Native traffic lights, macOS only. The generic TitleBar does
            // not know about them; they read the bar's visibility from the
            // shell's context and their two hosting facts from props.
            // Elsewhere they do not exist, so there is nothing to drive
            // and no command to invoke.
            {macos.then(|| {
                view! { <TrafficLights rail_hosted=rail_hosted bar_hosted=bar_hosted /> }
            })}
            {children()}
        </TitleBar>
    }
}
