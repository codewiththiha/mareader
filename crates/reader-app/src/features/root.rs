//! The reader's own surface: the rail, the viewer slot, and everything that
//! floats over the document.
//!
//! This is the half of the old `ReaderPage` that is DOCUMENT-shaped — the
//! half the reader build mounts for itself in Phase 2. The window chrome
//! around it (title bar, menus, settings modal, the Library button) stays in
//! the shell's route (`ReaderRoute`, `src/features/reader/route.rs`), which
//! builds the [`ShellController`], installs the reader's effects in their
//! one legal order, and hands this component the state slices it reads:
//! the reader's own signals, the settings snapshot, the library's covers.
//! Nothing here reaches back for the shell — a needs-the-shell moment is a
//! prop or a context, never an import.

use leptos::prelude::*;

use app_chrome::controller::ShellController;
use app_chrome::hooks::dom::VIEWER_SLOT_ID;
use pdf_engine::types::DocStatus;
use reader_core::settings::PageIndicatorStyle;

use crate::components::ai::gloss::gloss_ai_popover::GlossAiPopover;
use crate::components::ai::selection_pill::SelectionPill;
use crate::components::rail::overlay::OverlayRail;
use crate::components::rail::push::PushRail;
use crate::components::search::floating_search::FloatingSearch;
use crate::components::viewer::controls::bottom_bar::ReaderBottomBar;
use crate::components::viewer::controls::page_indicator::PageIndicator;
use crate::components::viewer::floating_document_title::FloatingDocumentTitle;
use crate::components::viewer::Viewer;
use crate::features::rail::ReaderRail;
use crate::state::ReaderState;
use reader_core::settings::Settings;
use reader_core::ui::SidebarMode;

/// The reader's surface, below the title bar: the docked rail as a flex
/// sibling, the `<main id=VIEWER_SLOT_ID>` viewer slot with everything that
/// floats over it, and the floating rail outside `.reader-bg`'s stacking
/// context. View only — the effects that feed these components are installed
/// by the route that mounts this one, in the order documented there.
#[component]
pub fn ReaderRoot(
    /// The reader's slice: every signal the viewer, the rail and the
    /// floating surfaces read and write.
    reader: ReaderState,
    /// The persisted settings snapshot the view reads (indicator toggles,
    /// page shadow, blend). Written by the shell; read here.
    settings: RwSignal<Settings>,
    /// The library's cover map, for the rail's identity row.
    covers: RwSignal<library_core::covers::CoverMap>,
    /// The shell layout truth, built once by the route and provided as
    /// context for every chrome component below.
    shell: ShellController,
    /// The route's virtualizer views (the `Clone`-safe `StoredValue`
    /// handles); the raw handles stay at the route with the effects.
    virtualizer_view: StoredValue<virtual_list_leptos::Virtualizer, LocalStorage>,
    h_virtualizer_view: StoredValue<virtual_list_leptos::Virtualizer, LocalStorage>,
) -> impl IntoView {
    let vs = reader;
    let sidebar = shell.sidebar_mode;

    // Text documents have no thumbnails — the engine never sees them. If
    // one opens while the rail is ON the Thumbs panel, move the rail to
    // the Outline panel (which degrades gracefully to its empty state) so
    // the reader never faces a panel that cannot show anything.
    Effect::new(move |_| {
        if vs.reflowable() && sidebar.get_untracked() == SidebarMode::Thumbs {
            sidebar.set(SidebarMode::Outline);
        }
    });

    let status = vs.document.status;
    let is_ready = move || status.get() == DocStatus::Ready;
    let show_indicator = Signal::derive(move || settings.with(|st| st.layout.page_indicator));
    let indicator_style = Signal::derive(move || settings.with(|st| st.layout.page_indicator_style));
    let progress_visible = Signal::derive(move || settings.with(|st| st.layout.progress_bar));
    // Continuous text reading has no meaningful page number: while the stream
    // is live the badge is a percentage of the document whatever the indicator
    // style says — and the style selector stands disabled for exactly as long,
    // so it cannot show a choice that is not being honoured.
    let stream_live = Signal::derive(move || vs.reflow_streaming());
    let stream_percent = Signal::derive(move || vs.stream_percent());

    view! {
        <div
            class="reader-bg relative flex h-full w-full flex-col overflow-hidden text-ink"
            class=("blend", move || {
                // The blend class swaps the backdrop AND the page hosts
                // onto the engine's one computed paper colour
                // (styles/components/shell.css, styles/page_host.css),
                // killing the fractional-edge rim a second paper colour
                // under the canvas used to show. A text/Markdown page is
                // its OWN paper (the surface paints --tx-paper), so the
                // class must not run for it or a second paper colour
                // stacks under the text page.
                settings.with(|st| st.layout.blend_mode) && !vs.reflowable()
            })
        >
            <div class="relative flex min-h-0 flex-1">
                // DOCKED: the rail is a flex sibling of `<main>`, so the
                // page gives up the width. `PushRail` renders nothing
                // while the controller says the layout is overlay.
                <PushRail shell=shell>
                    <ReaderRail reader=vs covers=covers shell=shell />
                </PushRail>
                <main
                    id=VIEWER_SLOT_ID
                    class="relative min-w-0 flex-1 overflow-hidden"
                    class=("no-page-shadow", move || !settings.with(|st| st.layout.page_shadow))
                >
                    <Show when=is_ready>
                        <Viewer
                            state=vs
                            virtualizer=virtualizer_view.get_value()
                            h_virtualizer=h_virtualizer_view.get_value()
                            progress_visible=progress_visible
                        />
                    </Show>
                    // The first-paint cover (the gate effects own
                    // its timing): an opaque sheet of the paper the reader
                    // is about to paint, over everything the viewer slot
                    // stacks, until the reading surface has landed on the
                    // resume point. Lifting is seamless in every theme
                    // because it wears the same paper token as the surface
                    // underneath.
                    <Show when=move || is_ready() && !vs.viewer.first_paint.get()>
                        <div
                            class=format!(
                                "absolute inset-0 {} flex items-center justify-center",
                                app_chrome::layers::DRAG_OVERLAY
                            )
                            style=move || format!(
                                "background:{}",
                                if vs.reflowable() {
                                    "var(--tx-paper)"
                                } else {
                                    "var(--color-paper)"
                                }
                            )
                        >
                            <ui_kit::primitives::feedback::CenteredLoader />
                        </div>
                    </Show>
                    <FloatingDocumentTitle reader=vs settings=settings />
                    // Corner page counter, gated on a ready document and
                    // positioned by the page; the indicator itself is
                    // reusable UI with no knowledge of the reader state.
                    <Show when=move || is_ready() && show_indicator.get()>
                        <div class=format!("pointer-events-none absolute bottom-3 right-3 {}", app_chrome::layers::CONTROLS)>
                            <PageIndicator
                                current=Signal::derive(move || {
                                    if stream_live.get() {
                                        stream_percent.get()
                                    } else {
                                        vs.viewer.page.get()
                                    }
                                })
                                total=Signal::derive(move || {
                                    if stream_live.get() {
                                        100
                                    } else {
                                        vs.document.num_pages.get()
                                    }
                                })
                                style=Signal::derive(move || {
                                    if stream_live.get() {
                                        PageIndicatorStyle::Percentage
                                    } else {
                                        indicator_style.get()
                                    }
                                })
                                hidden=Signal::derive(move || vs.gloss.selection_active.get())
                            />
                        </div>
                    </Show>
                    <ReaderBottomBar
                        reader=vs
                    />
                    <FloatingSearch
                        state=vs
                        virtualizer=virtualizer_view
                    />
                    <SelectionPill reader=vs />
                    <GlossAiPopover reader=vs settings=settings />
                </main>
            </div>
        </div>
        // OVERLAY: `OverlayRail` mounts OUTSIDE `.reader-bg`, which is a
        // stacking context at z-index 0 — a rail inside it would paint
        // under the title bar's band and hand the band its whole 48px
        // header (close, search, More, and the native traffic lights the
        // header's 88px gutter reserves). Out here its own z-popover
        // outranks the bar, so the rail covers the bar's left corner
        // (Library button included) and takes the lights with it, and the
        // bar reads as one full-width surface either way. Renders nothing
        // while the controller says the layout is docked.
        <OverlayRail shell=shell>
            <ReaderRail reader=vs covers=covers shell=shell />
        </OverlayRail>
    }
}
