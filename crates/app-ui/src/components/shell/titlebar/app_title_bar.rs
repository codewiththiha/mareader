//! The app's title bar: the generic [`TitleBar`] fed the controller's
//! answers as props.

use leptos::children::ViewFn;
use leptos::prelude::*;

use app_chrome::platform::{is_macos, uses_frameless_controls};
use app_chrome::titlebar::root::TitleBar;
use app_chrome::window::WindowControls;
use app_chrome::window::traffic_lights::TrafficLights;

use crate::components::shell::controller::ShellController;
use app_state::ChromeState;

#[component]
pub fn AppTitleBar(
    state: ChromeState,
    #[prop(into)] left: ViewFn,
    /// Centered overlay passed through to the generic title bar.
    #[prop(into, default = ViewFn::from(|| ()))]
    center: ViewFn,
    #[prop(into)] right: ViewFn,
    children: Children,
) -> impl IntoView {
    // The page provides the shell controller; the library's is rail-less.
    let shell = use_context::<ShellController>().expect("the page provides the shell controller");
    let pinned = shell.titlebar_pinned;
    let on_pin_change = Callback::new(move |p: bool| shell.set_titlebar_pinned(p));
    // The open floating search holds the bar (like an open popover).
    let extra_hold = Signal::derive(move || state.reader.search_visible.get());

    // Window chrome is platform-split: macOS native lights, Windows and
    // Linux frameless.
    let macos = is_macos();
    let frameless = uses_frameless_controls();
    if frameless {
        super::window_state::install(state.ui.window_maximized);
    }
    // The lights' two hosts, as the controller computes them.
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
            // Native traffic lights, macOS only.
            {macos.then(|| {
                view! { <TrafficLights rail_hosted=rail_hosted bar_hosted=bar_hosted /> }
            })}
            {children()}
        </TitleBar>
    }
}
