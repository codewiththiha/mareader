//! The reading page: title bar, rails, menus, and an empty slot.
//!
//! The book is not in this tree. A format instance mounts into `#viewer-slot`
//! and reports a snapshot; this page paints the chrome from that copy. It does
//! not mount a viewer, and it does not install the document effects.

use leptos::prelude::*;

use crate::components::menus::appearance_menu::AppearanceMenu;
use crate::components::menus::reader_menu::ReaderMenu;
use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::settings::modal::SettingsModal;
use crate::components::shell::controller::ShellController;
use crate::components::shell::sidebar::overlay::OverlayRail;
use crate::components::shell::sidebar::push::PushRail;
use crate::components::shell::titlebar::app_title_bar::AppTitleBar;
use crate::components::shell::titlebar::document_title::CenteredDocTitle;
use crate::components::shell::titlebar::floating_document_title::FloatingDocumentTitle;
use crate::features::reader::rail::ReaderRail;
use crate::services::document::close_document;
use crate::state::AppState;
use app_chrome::hooks::dom::{TOOLBAR_LEADING_ID, VIEWER_SLOT_ID};
use app_chrome::icon::{Icon, IconName};
use app_chrome::tooltip::Tooltip;
use pdf_engine::types::DocStatus;

#[component]
pub fn ReaderPage(state: AppState) -> impl IntoView {
    let shell = ShellController::reader(state);
    provide_context(shell);

    // Before the first mount, so a snapshot that arrives with the instance is
    // not dropped on the floor. The effect waits until this view has committed
    // `#viewer-slot`; applying from the session setup raced that div.
    crate::slot::install(state);
    Effect::new(move |_| {
        crate::boot::apply_handoff(state);
    });

    let settings_open = RwSignal::new(false);
    provide_context(settings_open);

    let left = move || {
        view! {
            <div
                id=TOOLBAR_LEADING_ID
                data-tauri-drag-region="true"
                class="flex shrink-0 items-center gap-1"
            >
                <Show when=move || shell.show_sidebar_toggle().get()>
                    <Tooltip text="Toggle sidebar">
                        <Button
                            on_click=move |_| shell.toggle_sidebar()
                            variant=ButtonVariant::Ghost
                            title="Toggle sidebar"
                        >
                            <Icon name=IconName::Sidebar size=18 />
                        </Button>
                    </Tooltip>
                </Show>
                <Show when=move || {
                    matches!(
                        state.reader.document.status.get(),
                        DocStatus::Ready | DocStatus::Opening | DocStatus::Error
                    )
                }>
                    <Tooltip text="Library">
                        <Button
                            on_click=move |_| close_document(state)
                            variant=ButtonVariant::Ghost
                            title="Close this book and return to the library"
                        >
                            <Icon name=IconName::Library size=18 />
                        </Button>
                    </Tooltip>
                </Show>
            </div>
        }
    };
    let center = move || view! { <CenteredDocTitle state=state /> };
    let right = move || {
        view! {
            <ReaderMenu state=state settings_open=settings_open />
            <AppearanceMenu state=state />
        }
    };

    view! {
        <AppTitleBar state=state left=left center=center right=right>
            <div
                class="reader-bg relative flex h-full w-full flex-col overflow-hidden text-ink"
                class=("blend", move || {
                    state.settings.with(|st| st.layout.blend_mode)
                        && !state.reader.reflowable()
                })
            >
                <div class="relative flex min-h-0 flex-1">
                    <PushRail shell=shell>
                        <ReaderRail state=state shell=shell />
                    </PushRail>
                    <main
                        id=VIEWER_SLOT_ID
                        class="relative min-w-0 flex-1 overflow-hidden"
                        class=("no-page-shadow", move || !state.settings.with(|st| st.layout.page_shadow))
                    >
                        <Show when=move || state.reader.document.status.get() == DocStatus::Opening>
                            <div class=format!(
                                "absolute inset-0 {} flex items-center justify-center bg-paper",
                                app_chrome::layers::DRAG_OVERLAY
                            )>
                                <crate::components::primitives::feedback::CenteredLoader />
                            </div>
                        </Show>
                        <Show when=move || state.reader.document.status.get() == DocStatus::Error>
                            <div class=format!(
                                "absolute inset-0 {} flex items-center justify-center px-8 text-center text-muted",
                                app_chrome::layers::DRAG_OVERLAY
                            )>
                                <p class="text-lg">
                                    {move || {
                                        state.reader.document.error.get().unwrap_or_else(|| {
                                            "Could not open this document".to_string()
                                        })
                                    }}
                                </p>
                            </div>
                        </Show>
                        <FloatingDocumentTitle state=state />
                    </main>
                </div>
            </div>
            <OverlayRail shell=shell>
                <ReaderRail state=state shell=shell />
            </OverlayRail>
            <SettingsModal state=state open=settings_open />
        </AppTitleBar>
    }
}
