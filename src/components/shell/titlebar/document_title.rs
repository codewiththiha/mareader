//! The toolbar document-name label. Its width is the actual space between
//! the leading controls and trailing toolbar cluster; it deliberately
//! measures DOM rects rather than reproducing title-bar padding or
//! sidebar-state assumptions.

use leptos::prelude::*;

use crate::components::primitives::hooks::dom::{
    by_id, by_id_warn, TOOLBAR_LEADING_ID, TOOLBAR_ROW_ID, TOOLBAR_TRAILING_ID,
};
use crate::components::primitives::hooks::use_resize_observer::observe_elements;
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
    // Warn-once lookups: while the reader is mounted these ids always exist,
    // so a miss is a renamed id (or a stale rAF after unmount) and deserves
    // one console line rather than silence.
    let row = by_id_warn(TOOLBAR_ROW_ID)?;
    let row_rect = row.get_bounding_client_rect();
    if row_rect.width() <= 0.0 {
        return None;
    }
    let pre = by_id_warn(TOOLBAR_LEADING_ID)?;
    let pre_rect = pre.get_bounding_client_rect();
    let right = by_id_warn(TOOLBAR_TRAILING_ID)?;
    let right_rect = right.get_bounding_client_rect();

    // The pin button follows #toolbar-trailing inside its ml-auto parent. Using
    // the trailing group's left edge therefore reserves it automatically.
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
                // The rAF can outlive this component (a route flip back to the
                // library disposes `avail` while a frame is in flight), so a
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
/// The label carries `data-tauri-drag-region` itself, not just the band behind
/// it. The label is the most obvious thing to grab in a bar whose whole job is
/// holding the window title, and it is exactly the thing `-webkit-user-select:
/// none` makes unselectable — so it must move the window instead of doing
/// nothing. Whether Tauri's handler reads the attribute off the event's target
/// (the per-element style every other drag region in this app uses: the band,
/// the row, the leading and trailing clusters all carry it) or resolves the
/// nearest ancestor, naming it here is the version that cannot fail.
#[component]
pub fn CenteredDocTitle(state: AppState) -> impl IntoView {
    let name = move || state.reader.document.display_name();
    view! {
        <span
            data-tauri-drag-region="true"
            class="max-w-[46vw] truncate text-sm font-medium text-ink"
            title=name
        >
            {name}
        </span>
    }
}
