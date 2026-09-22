//! The `/reader` route: the app title bar around the reader's own root, plus
//! the settings modal the window owns.
//!
//! The split along this seam is the whole point of the file. Everything that
//! is true of an OPEN DOCUMENT — the rail, the viewer slot, the floating
//! surfaces over it, and the effects that keep them in sync — is
//! `reader_app`'s, mounted here as one component. What is left is what is
//! true of a WINDOW: the bar and its three clusters, the Library button that
//! closes the document, the settings modal, and the two facts the reader
//! cannot own (the persisted settings, and where a reading position is
//! written).
//!
//! Slot wiring is the SINGLE coordinator's job — branches must not edit this
//! file. The shell's layout truth lives in one `ShellController` built here
//! and provided as context; the rail's two mount points (`PushRail` in the
//! flex row, `OverlayRail` above the reader surface) and every chrome
//! component ask it instead of recomputing layout facts.

use leptos::prelude::*;

use ai_core::gloss::GlossMark;
use app_chrome::controller::ChromeSurface;
use app_chrome::hooks::dom::TOOLBAR_LEADING_ID;
use app_chrome::icon::{Icon, IconName};
use app_chrome::tooltip::Tooltip;
use pdf_engine::types::DocStatus;
use reader_app::state::GlossSave;
use ui_kit::controls::button::{Button, ButtonVariant};

use crate::components::menus::appearance_menu::AppearanceMenu;
use crate::components::menus::reader_menu::ReaderMenu;
use crate::components::settings::modal::SettingsModal;
use crate::components::shell::controller_for;
use crate::components::shell::titlebar::app_title_bar::AppTitleBar;
use crate::components::shell::titlebar::document_title::CenteredDocTitle;
use crate::effects::app::reading_progress::reading_progress;
use crate::services::document::close_document;
use crate::state::AppState;

#[component]
pub fn ReaderPage(state: AppState) -> impl IntoView {
    // The viewer slice of app state, handed to the reader's own root and to
    // every component and effect inside it.
    let vs = state.reader;

    // The shell's layout brain: one controller for the whole page, provided as
    // context for the title bar, the traffic lights, the floating label and
    // both rail mount points. It owns the open/close slide machine, so the
    // chrome stays aligned with the rail's pixels for the whole slide.
    let shell = controller_for(state, ChromeSurface::Reader);
    provide_context(shell);

    let settings_open = RwSignal::new(false);
    // The settings modal is opened from several places (the 3-dash menu's
    // Settings… item, the sidebar header's gear) that sit under different
    // mount points, so the open signal is shared through context rather than
    // threaded as a prop through the rail composition.
    provide_context(settings_open);

    // The reader's one write to storage, injected rather than reached for:
    // the marks land in the library's blob, which is this crate's to own. See
    // `reader_app::state::GlossSave`.
    provide_context::<GlossSave>(Callback::new(move |(key, marks): (String, Vec<GlossMark>)| {
        crate::storage::persist_gloss(&key, &marks)
    }));

    // Left: sidebar toggle + Library; title centered; right: the 3-dash view
    // menu + Appearance.
    //
    // The sidebar toggle's visibility is the controller's rule: overlay mode
    // drops it (the rail opens by brushing the window's left edge and closes
    // from its own header, so a second switch in the bar only competes with
    // both). The Library button stays put — the rail floats above the bar and
    // covers it while up, which is the rail's job. The cluster is always
    // mounted so the row keeps its left edge (and `#toolbar-leading`, the
    // measurement anchor the library title uses) wherever the mode puts it.
    // Reader settings have no button of their own: they open from the 3-dash
    // menu's Settings item and the sidebar header's gear.
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
                        DocStatus::Ready | DocStatus::Opening
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
            <reader_app::features::ReaderRoot
                reader=vs
                settings=state.settings
                covers=state.library.covers
                shell
                // Installed by the reader root, at the point in its effect
                // order the zoom contract asks for: after `navigation_sync`,
                // so a closing zoom transaction replays its held jump before
                // the page this persists is read. It is the shell's effect —
                // it writes the library's rows and their blob — running on the
                // reader's clock.
                record_progress=Callback::new(move |_| reading_progress(state))
            />
            // The settings modal belongs to the window, not to the viewer: as a
            // child of `main` it sat inside `.reader-bg`, which is a stacking
            // context (position:relative + z-index:0), so the title bar's band
            // — a SIBLING of `.reader-bg` at z-bar — painted over the top of an
            // open modal. As a sibling of the reader root, its own z-popover
            // token outranks the bar, which is what a modal is supposed to do.
            // It also renders after the floating rail, so it wins their shared
            // z-popover token and still covers it.
            <SettingsModal state=state open=settings_open />
        </AppTitleBar>
    }
}
