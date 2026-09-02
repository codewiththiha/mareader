//! The toolbar document-name labels, one per page:
//!
//! - [`DocumentTitle`] (library) sits in the leading cluster and measures
//!   its own max-width against the trailing cluster;
//! - [`CenteredDocTitle`] (reader) sits in the shell's center slot, whose
//!   box the shell resolves — the exact row center while the label fits,
//!   the free stretch between the clusters once it does not.
//!
//! Both measure live DOM rects rather than reproducing title-bar padding
//! or sidebar-state assumptions.

use leptos::prelude::*;

use app_chrome::hooks::dom::{
    by_id, by_id_warn, TOOLBAR_CENTER_TITLE_ID, TOOLBAR_LEADING_ID, TOOLBAR_ROW_ID,
    TOOLBAR_TRAILING_ID,
};
use app_chrome::hooks::use_resize_observer::observe_elements;
use crate::components::shell::sidebar::container::SIDEBAR_ASIDE_SELECTOR;

use crate::state::AppState;

/// Gap after the leading controls (`gap-1`).
const GAP_LEFT: f64 = 4.0;
/// Breathing room before the trailing control cluster.
const GAP_RIGHT: f64 = 12.0;
const TITLE_MIN_LABEL_W: f64 = crate::components::shell::titlebar::constants::MIN_DOC_TITLE_WIDTH;

/// Measure the title's real slot in toolbar-row coordinates. This remains
/// correct through the sidebar close slide because it uses the live rects,
/// not the raw sidebar mode or title-bar padding model.
fn measure_available() -> Option<f64> {
    // Warn-once lookups: while the library page is mounted these ids always
    // exist, so a miss is a renamed id (or a stale rAF after unmount) and
    // deserves one console line rather than silence.
    let row = by_id_warn(TOOLBAR_ROW_ID)?;
    let row_rect = row.get_bounding_client_rect();
    if row_rect.width() <= 0.0 {
        return None;
    }
    let pre = by_id_warn(TOOLBAR_LEADING_ID)?;
    let pre_rect = pre.get_bounding_client_rect();
    let right = by_id_warn(TOOLBAR_TRAILING_ID)?;
    let right_rect = right.get_bounding_client_rect();

    // #toolbar-trailing is the shell's trailing group (right cluster + pin),
    // so its left edge reserves both automatically.
    let start = pre_rect.right() - row_rect.left() + GAP_LEFT;
    let end = right_rect.left() - row_rect.left() - GAP_RIGHT;
    Some((end - start).max(0.0))
}

#[component]
pub fn DocumentTitle(state: AppState) -> impl IntoView {
    let avail = RwSignal::new(None::<f64>);

    let remeasure = move || {
        request_animation_frame(move || {
            if let Some(w) = measure_available() {
                // The rAF can outlive this component (a route flip to the
                // reader disposes `avail` while a frame is in flight), so a
                // plain read would panic on the disposed signal. Try-accessors
                // make a stale frame a silent no-op.
                let prev = avail.try_get_untracked().flatten();
                if prev.is_none_or(|p: f64| (p - w).abs() > 0.5) {
                    let _ = avail.try_set(Some(w));
                }
            }
        });
    };

    // The route/page mount order may put this effect ahead of its anchor
    // ids; the primitive installs once with the first non-empty set and a
    // later reactive run self-heals an initial miss.
    Effect::new(move |_| {
        // Silent lookups on purpose: the effect can run before its anchor
        // ids have mounted, and the observer primitive self-heals an initial
        // miss on a later reactive run (see its docs). Warn-once would turn
        // that ordinary mount race into console noise.
        let mut els = Vec::new();
        for id in [TOOLBAR_ROW_ID, TOOLBAR_LEADING_ID, TOOLBAR_TRAILING_ID] {
            if let Some(el) = by_id(id) {
                els.push(el);
            }
        }
        // The title row changes inset at the end of the close hold, whereas
        // this aside changes width during every frame of the 300ms slide.
        if let Some(aside) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.query_selector(SIDEBAR_ASIDE_SELECTOR).ok().flatten())
        {
            els.push(aside);
        }
        observe_elements(els, move |_| remeasure());
    });

    Effect::new(move |_| {
        _ = state.reader.document.status.get();
        _ = state.reader.document.num_pages.get();
        _ = state.reader.document.title.get();
        _ = state.reader.document.path.get();
        _ = state.ui.sidebar.get();
        remeasure();
    });

    let name = move || state.reader.document.display_name();
    let full = move || name();
    let hidden = move || avail.get().is_some_and(|w| w < TITLE_MIN_LABEL_W);

    view! {
        <span
            id="toolbar-doc-title"
            data-tauri-drag-region="true"
            class="min-w-0 shrink truncate text-sm text-ink"
            class=("hidden", hidden)
            title=full
            style:max-width=move || match avail.get() {
                Some(w) if w >= TITLE_MIN_LABEL_W => format!("{}px", w.floor()),
                Some(_) => "0px".to_string(),
                None => "none".to_string(),
            }
        >
            {name}
        </span>
    }
}

/// Centered document name for the reader title bar's center slot.
///
/// The shell (`TitleBar` in app-chrome) owns position and width: it
/// measures the row, both clusters and this label's natural width (the
/// `#toolbar-center-title` anchor), then places the label at the row's
/// EXACT center while it fits, falling back to the free stretch between
/// the clusters (centered, truncated) once it does not — with the
/// caption cluster, the pin and the traffic-light gutter reserved on
/// every platform. Below the shell's floor the slot hides the label
/// rather than showing a stub.
///
/// The label carries `data-tauri-drag-region` itself: it is the most
/// obvious thing to grab in a bar whose job is holding the window title,
/// and `-webkit-user-select: none` makes it unselectable — so it must
/// move the window instead of doing nothing.
#[component]
pub fn CenteredDocTitle(state: AppState) -> impl IntoView {
    let center_title_ref =
        expect_context::<app_chrome::titlebar::root::TitleBarCtx>().center_title_ref;
    let name = move || state.reader.document.display_name();
    view! {
        <span
            node_ref=center_title_ref
            // The shell's natural-width anchor (its scrollWidth) for the
            // center-slot decision, observed to re-measure on renames.
            // pointer-events-auto: the slot overlay is click-transparent,
            // but the label itself must still grab the window and show
            // its tooltip.
            id=TOOLBAR_CENTER_TITLE_ID
            data-tauri-drag-region="true"
            class="pointer-events-auto min-w-0 max-w-full truncate text-sm font-medium text-ink"
            title=name
        >
            {name}
        </span>
    }
}
