//! Top toolbar. OWNED BY branch B (viewer/chrome).
//! Redesigned (U2): hamburger + filename on the left, true viewport-centered
//! page nav (Single mode only). The right group is the U7 audit layout:
//! search + segmented Single/Continuous + zoom, then a single Appearance menu
//! (U6) and a More (⋯) overflow menu. The sidebar panel toggles were removed —
//! the sidebar's own tab rail is the single source of truth for which panel is
//! open.


use leptos::prelude::*;

use pdf_viewer::components::atoms::button::{Button, ButtonKind};
use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::components::atoms::segmented::{Segmented, SegmentedLabel};
use pdf_viewer::components::atoms::separator::Separator;
use pdf_viewer::components::atoms::tooltip::Tooltip;
use pdf_engine::types::DocStatus;
use pdf_core::layout::ViewMode;
use crate::core::open_flow::{close_document, open_dialog};
use crate::core::state::AppState;
use pdf_viewer::state::SidebarMode;

use super::appearance_menu::AppearanceMenu;
use super::doc_title::DocTitle;
use super::more_menu::MoreMenu;
use pdf_viewer::components::page_nav::PageNav;
use super::zoom_controls::ZoomControls;

#[component]
pub fn Toolbar(state: AppState) -> impl IntoView {
    let mode = state.viewer.mode;
    let viewer_state = pdf_viewer::state::ViewerState::new(state.doc, state.viewer, state.search, state.sidebar);

    let open_state = state;
    let menu_state = state;
    let mode_state = state;
    let filename_state = state;

    view! {
        // `#toolbar-row` and the `#toolbar-*` ids below are MEASUREMENT
        // ANCHORS for molecules::doc_title, which sizes the document-name label
        // from the real widths of the groups around it so the name is only
        // folded with `…` when it would truly collide. Renaming or removing one
        // silently degrades the label to "never truncate" (see doc_title.rs).
        <div id="toolbar-row" class="relative flex h-12 items-center gap-2 px-3">
            // LEFT GROUP: hamburger + Open + filename.
            <div class="flex min-w-0 items-center gap-1">
                // Fixed-size controls left of the name, measured as one unit.
                <div id="toolbar-left-pre" class="flex shrink-0 items-center gap-1">
                // Sidebar toggle only while a document is open: the home screen
                // (library / empty state) has no outline or thumbnails to show,
                // so the hamburger would open an empty panel.
                <Show when=move || menu_state.doc.status.get() == DocStatus::Ready>
                    <Tooltip text="Toggle sidebar".to_string()>
                        <Button
                            on_click=move |_| {
                                let next = if menu_state.sidebar.get() == SidebarMode::None {
                                    SidebarMode::Thumbs
                                } else {
                                    SidebarMode::None
                                };
                                menu_state.sidebar.set(next);
                            }
                            kind=ButtonKind::Ghost
                            icon=IconName::Menu
                            title="Toggle sidebar".to_string()
                        />
                    </Tooltip>
                </Show>
                // Back to the library shelf. Only while a document is open —
                // the shelf IS the empty state, so the button would be a no-op
                // (and confusing) there.
                <Show when=move || menu_state.doc.status.get() == DocStatus::Ready>
                    <Tooltip text="Library".to_string()>
                        <Button
                            on_click=move |_| close_document(menu_state)
                            kind=ButtonKind::Ghost
                            icon=IconName::Library
                            title="Close this book and return to the library".to_string()
                        />
                    </Tooltip>
                </Show>
                <Tooltip text="Open PDF (Cmd/Ctrl+O)".to_string()>
                    <Button
                        on_click=move |_| open_dialog(open_state)
                        kind=ButtonKind::Toolbar
                        icon=IconName::Open
                        label="Open".to_string()
                        title="Open PDF (Cmd/Ctrl+O)".to_string()
                    />
                </Tooltip>
                </div>
                // Document name. Self-measuring: it folds with `…` ONLY when it
                // would otherwise collide with the centered page nav (single
                // mode) or the right-hand controls, and hides itself entirely
                // when the window is too narrow for any useful name.
                <DocTitle state=filename_state />
            </div>

            // CENTER: absolutely positioned, TRUE viewport centering (Single
            // mode only; the self-sized wrapper stays out of the left/right
            // groups' way). `#toolbar-center` is a doc_title measurement anchor:
            // its PRESENCE is how the label knows the centered nav is in play.
            <Show when=move || mode.get() == ViewMode::Single>
                <div
                    id="toolbar-center"
                    class="absolute left-1/2 top-1/2 z-10 -translate-x-1/2 -translate-y-1/2"
                >
                    <PageNav state=viewer_state />
                </div>
            </Show>

            // RIGHT GROUP.
            <div id="toolbar-right" class="ml-auto flex shrink-0 items-center gap-1">
                // Floating-search toggle (U4): lets mouse-only users open search
                // between Phase 1 and Phase 3; Cmd/Ctrl+F does the same. A raw
                // button (not the Button atom) so pointerdown can stop
                // propagation: the floating bar's outside-click dismiss listens
                // on window pointerdown, which would otherwise close the bar and
                // then the click would re-open it — making the toggle one-way.
                <Tooltip text="Search (Cmd/Ctrl+F)".to_string()>
                    <button
                        type="button"
                        // Marks this as search chrome: a dismissed search's
                        // muted highlights survive a click here, because
                        // reaching for the search button is coming BACK to the
                        // search rather than moving on from it.
                        data-search-chrome="true"
                        title="Search (Cmd/Ctrl+F)"
                        on:pointerdown=move |ev| ev.stop_propagation()
                        on:click=move |_| {
                            if state.search.visible.get() {
                                pdf_viewer::effects::search_effects::dismiss_search(viewer_state);
                            } else {
                                pdf_viewer::effects::search_effects::resume_search(viewer_state);
                            }
                        }
                        class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-transparent bg-transparent text-ink transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    >
                        <Icon name=IconName::Search size=16 />
                    </button>
                </Tooltip>
                // Per-segment titles: the segments are icon-only, so without
                // them the two buttons are unlabelled for screen readers,
                // hover tooltips, and UI tests alike (the wrapping Tooltip only
                // titles the group, not the individual options).
                <Tooltip text="View mode".to_string()>
                    <Segmented
                        options=vec![
                            (
                                ViewMode::Single,
                                SegmentedLabel::Icon(IconName::SinglePage),
                                "Single page view",
                            ),
                            (
                                ViewMode::Continuous,
                                SegmentedLabel::Icon(IconName::Continuous),
                                "Continuous scroll view",
                            ),
                        ]
                        value={mode.read_only()}
                        on_change=move |m: ViewMode| mode_state.viewer.mode.set(m)
                    />
                </Tooltip>
                <ZoomControls state=state />
                <Separator vertical=true />
                <AppearanceMenu state={state} />
                <MoreMenu state={state} />
            </div>
        </div>
    }
}
