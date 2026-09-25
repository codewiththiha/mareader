//! Centered reader settings modal: the tab strip and the body that hosts one tab
//! at a time. The tabs live in `layout`, `theme`, `animations` and `fonts`, and
//! the SET of them is not fixed — see `shown`.
//!
//! The backdrop, the panel, the overlay lane and the Escape rule are
//! [`ModalShell`]'s, which is what every other sheet in the app rides: one
//! contract for how a modal opens and closes rather than one per surface, and
//! the `role="dialog"` and `aria-label` that go with it. What is left here is
//! this modal's own face — a strip of tabs where a sheet has a heading.
//!
//! The `open` signal belongs to the page (two things open this modal: the
//! gear button and the reader menu's item), and the shell registers it, so
//! opening a menu closes the modal and vice versa without either component
//! knowing about the other — see
//! [`lanes`](crate::components::primitives::overlay::lanes).

use leptos::prelude::*;

use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::settings::animations::AnimationsTab;
use crate::components::settings::common::{Tab, TabButton};
use crate::components::settings::fonts::FontsTab;
use crate::components::settings::layout::LayoutTab;
use crate::components::settings::theme::ThemeTab;
use crate::state::AppState;
use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;

#[component]
pub fn SettingsModal(
    state: AppState,
    open: RwSignal<bool>,
    #[prop(default = "min(92vw, 620px)")] width: &'static str,
    #[prop(default = "min(76vh, 640px)")] height: &'static str,
) -> impl IntoView {
    let tab = RwSignal::new(Tab::Layout);
    // The Animations tab is offered only while the master switch in the Layout
    // tab is on — an animations panel that cannot animate anything is worse
    // than no panel. `shown` is the tab the strip displays and the body renders
    // (rather than an effect writing `tab` back): turning the master off while
    // that tab happens to be open falls back to Layout for as long as it is
    // off, and the reader's own selection survives to be returned to.
    let animations_on = Signal::derive(move || state.settings.with(|st| st.animations.enabled));
    // The Fonts tab exists only while a reflowable document is open — a
    // PDF carries none of the type it controls — and follows the same
    // fallback rule as the Animations tab: selected while a PDF opens over
    // it, the strip shows Layout, and the selection survives the return.
    let fonts_on = Signal::derive(move || state.reader.reflowable());
    let shown = Signal::derive(move || match tab.get() {
        Tab::Animations if !animations_on.get() => Tab::Layout,
        Tab::Fonts if !fonts_on.get() => Tab::Layout,
        other => other,
    });
    view! {
        <ModalShell open=open aria_label="Reader settings" width=width height=height>
            {move || {
                view! {
                    <>
                        <div class="flex shrink-0 items-center gap-1 px-4 pb-2 pt-4">
                            <TabButton
                                tab=tab
                                active=shown
                                t=Tab::Layout
                                icon=IconName::Layout
                                label="Layout"
                            />
                            <TabButton
                                tab=tab
                                active=shown
                                t=Tab::Theme
                                icon=IconName::Palette
                                label="Theme"
                            />
                            <Show when=move || animations_on.get()>
                                <TabButton
                                    tab=tab
                                    active=shown
                                    t=Tab::Animations
                                    icon=IconName::Motion
                                    label="Animations"
                                />
                            </Show>
                            <Show when=move || fonts_on.get()>
                                <TabButton
                                    tab=tab
                                    active=shown
                                    t=Tab::Fonts
                                    icon=IconName::Type
                                    label="Fonts"
                                />
                            </Show>
                            <div class="ml-auto">
                                <IconButton
                                    icon=IconName::Close
                                    title="Close"
                                    class="rounded-full bg-line/60 hover:bg-line".to_string()
                                    on_click=move || open.set(false)
                                />
                            </div>
                        </div>
                        <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-5">
                            {move || match shown.get() {
                                Tab::Layout => view! { <LayoutTab state=state /> }.into_any(),
                                Tab::Theme => view! { <ThemeTab state=state /> }.into_any(),
                                Tab::Animations => {
                                    view! { <AnimationsTab state=state /> }.into_any()
                                }
                                Tab::Fonts => view! { <FontsTab state=state /> }.into_any(),
                            }}
                        </div>
                    </>
                }
            }}
        </ModalShell>
    }
}
