//! The centered settings modal: the tab strip and one tab's body.

use leptos::prelude::*;

use crate::components::settings::animations::AnimationsTab;
use crate::components::settings::common::{Tab, TabButton};
use crate::components::settings::fonts::FontsTab;
use crate::components::settings::layout::LayoutTab;
use crate::components::settings::theme::ThemeTab;
use crate::components::settings::workspace::WorkspaceTab;
use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;
use app_ui::components::primitives::overlay::modal_shell::ModalShell;

#[component]
pub fn SettingsModal(
    state: crate::context::ReaderContext,
    open: RwSignal<bool>,
    #[prop(default = "min(92vw, 620px)")] width: &'static str,
    #[prop(default = "min(76vh, 640px)")] height: &'static str,
) -> impl IntoView {
    let tab = RwSignal::new(Tab::Layout);
    // The Animations tab exists only while its master switch is on.
    let animations_on = Signal::derive(move || state.settings.with(|st| st.animations.enabled));
    // The Fonts tab exists only for a reflowable document.
    let fonts_on = Signal::derive(move || state.reader.reflowable());
    let shown = Signal::derive(move || match tab.get() {
        Tab::Animations if !animations_on.get() => Tab::Layout,
        Tab::Fonts if !fonts_on.get() => Tab::Layout,
        other => other,
    });
    let state_store = StoredValue::new(state);

    view! {
        <ModalShell open=open aria_label="Reader settings" width=width height=height>
            {move || {
                let state = state_store.get_value();
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
                            <TabButton
                                tab=tab
                                active=shown
                                t=Tab::Workspace
                                icon=IconName::SplitRight
                                label="Workspace"
                            />
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
                                Tab::Workspace => {
                                    view! { <WorkspaceTab state=state /> }.into_any()
                                }
                            }}
                        </div>
                    </>
                }
            }}
        </ModalShell>
    }
}
