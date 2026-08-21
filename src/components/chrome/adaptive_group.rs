//! Collision-aware toolbar group. Items that don't fit move into a
//! conditional "…" overflow popover; with room to spare the button vanishes.
use std::sync::Arc;
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;
use pdf_engine::types::DocStatus;
use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::dom::by_id;
use crate::components::chrome::popover::Popover;
use crate::core::state::AppState;

pub const TB_GAP: f64 = 4.0;          // gap-1
pub const TB_OVERFLOW_W: f64 = 36.0;  // h-9 w-9
pub const TB_RIGHT_RESERVE: f64 = 48.0; // pr-2 + pin button (same contract as DocTitle)
pub const TB_TITLE_RESERVE: f64 = 56.0; // MIN_LABEL_W: promise the name this much

/// One control that can live in the bar or in the overflow menu.
#[derive(Clone)]
pub struct ToolbarEntry {
    pub id: &'static str,
    /// Higher survives longer. `u32::MAX` = essential, never collapses.
    pub priority: u32,
    /// Stay in the DOM while collapsed so a popover owner can still open.
    pub keep_mounted: bool,
    pub inline: Arc<dyn Fn() -> AnyView + Send + Sync>,
    /// Pure measurement twin: always renders at full uncollapsed width.
    pub sizer: Arc<dyn Fn() -> AnyView + Send + Sync>,
    /// Row shown inside the overflow menu; the callback closes the menu.
    pub collapsed: Arc<dyn Fn(Callback<()>) -> AnyView + Send + Sync>,
}

/// Indices that must move to the overflow menu. Pure + deterministic.
pub fn compute_collapsed(
    widths: &[f64], priorities: &[u32], capacity: f64, gap: f64, overflow_w: f64,
) -> Vec<usize> {
    let n = widths.len();
    if n == 0 { return vec![]; }
    let total: f64 = widths.iter().sum::<f64>() + gap * (n.saturating_sub(1) as f64);
    if total <= capacity { return vec![]; }
    // The "…" button itself will occupy space once anything collapses.
    let budget = capacity - overflow_w - gap;
    // Drop lowest priority first; ties drop the right-most UI item first.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| priorities[a].cmp(&priorities[b]).then(b.cmp(&a)));
    let mut dropped = vec![false; n];
    let mut used = total;
    for &i in &order {
        if priorities[i] == u32::MAX { continue; }
        if used <= budget { break; }
        used -= widths[i] + gap;
        dropped[i] = true;
    }
    (0..n).filter(|&i| dropped[i]).collect()
}

#[component]
pub fn AdaptiveGroup(
    state: AppState,
    entries: Vec<ToolbarEntry>,
    /// Shared collapsed-id list so popover owners can hide their trigger.
    collapsed_ids: RwSignal<Vec<&'static str>>,
    /// Overflow "…" wrapper; Appearance re-anchors here when collapsed.
    overflow_ref: NodeRef<html::Div>,
) -> impl IntoView {
    let entries = RwSignal::new(entries);
    let sizer_ref: NodeRef<html::Div> = NodeRef::new();
    let open = RwSignal::new(false);

    let recalc = move || {
        request_animation_frame(move || {
            let Some(sizer) = sizer_ref.get() else { return };
            let Some(row) = by_id("toolbar-row") else { return };
            let list = entries.get_untracked();
            let kids = sizer.children();
            let widths: Vec<f64> = (0..list.len() as u32)
                .map(|i| kids.item(i).map(|e| e.get_bounding_client_rect().width()).unwrap_or(0.0))
                .collect();
            let priorities: Vec<u32> = list.iter().map(|e| e.priority).collect();
            let rr = row.get_bounding_client_rect();
            let left_end = by_id("toolbar-left-pre")
                .map(|e| e.get_bounding_client_rect().right())
                .unwrap_or(rr.left());
            let title_reserve = if state.doc.status.get_untracked() == DocStatus::Ready {
                TB_TITLE_RESERVE
            } else { 0.0 };
            let start = left_end + TB_GAP + title_reserve + 12.0; // 12 = GAP_RIGHT
            let capacity = (rr.right() - TB_RIGHT_RESERVE - start - 12.0).max(0.0);
            let ids: Vec<&'static str> = compute_collapsed(&widths, &priorities, capacity, TB_GAP, TB_OVERFLOW_W)
                .iter().map(|&i| list[i].id).collect();
            if ids != collapsed_ids.get_untracked() {
                collapsed_ids.set(ids);
            }
        });
    };

    let observer_handle = StoredValue::new_local(None::<web_sys::ResizeObserver>);
    let callback_handle = StoredValue::new_local(None::<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>);
    Effect::new(move |_| {
        // Track the sizer node so this re-runs once it mounts.
        let Some(sizer) = sizer_ref.get() else { return };
        if callback_handle.with_value(|c| c.is_some()) { return; }
        let Some(row) = by_id("toolbar-row") else { return };
        let cb: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> =
            Closure::wrap(Box::new(move |_: Vec<ResizeObserverEntry>| recalc())
                as Box<dyn FnMut(Vec<ResizeObserverEntry>)>);
        let fn_ref: &js_sys::Function = cb.as_ref().unchecked_ref();
        let Ok(ro) = web_sys::ResizeObserver::new(fn_ref) else { return };
        ro.observe(&row);
        ro.observe(&sizer);
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            for id in ["toolbar-left-pre", "toolbar-right"] {
                if let Some(el) = doc.get_element_by_id(id) {
                    ro.observe(&el);
                }
            }
        }
        observer_handle.set_value(Some(ro));
        callback_handle.set_value(Some(cb));
        recalc();
    });
    Effect::new(move |_| {
        _ = state.sidebar.get();
        _ = state.doc.status.get();
        _ = state.doc.num_pages.get();
        _ = state.doc.title.get();
        _ = state.doc.path.get();
        recalc();
    });
    on_cleanup(move || {
        if let Some(ro) = observer_handle.try_get_value().flatten() { ro.disconnect(); }
        observer_handle.try_set_value(None);
        callback_handle.try_set_value(None);
    });

    view! {
        <div class="relative shrink-0">
            <div id="toolbar-right" data-tauri-drag-region="true" class="flex shrink-0 items-center gap-1">
                // ⋯ FIRST: it stands where the collapsed controls used to be.
                // When nothing has collapsed the trigger stays hidden via `<Show>`,
                // so this only occupies space once at least one entry was evicted.
                <Show when=move || !collapsed_ids.get().is_empty() || open.get()>
                    <div node_ref=overflow_ref class="relative inline-flex">
                        <button
                            type="button"
                            title="More tools"
                            on:click=move |_| open.set(!open.get())
                            class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-transparent bg-transparent text-ink transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        >
                            <Icon name=IconName::More size=18 />
                        </button>
                        <Popover open=open anchor=overflow_ref width=224 class="p-1".to_string()>
                            <For
                                each=move || collapsed_ids.get()
                                key=|id| *id
                                children=move |id| {
                                    let done = Callback::new(move |_| open.set(false));
                                    entries
                                        .get_untracked()
                                        .into_iter()
                                        .find(|en| en.id == id)
                                        .map(|en| (en.collapsed)(done))
                                        .unwrap_or_else(|| ().into_any())
                                }
                            />
                        </Popover>
                    </div>
                </Show>
                // Surviving entries AFTER the ⋯.
                <For
                    each=move || {
                        let c = collapsed_ids.get();
                        entries
                            .get()
                            .into_iter()
                            .filter(|en| en.keep_mounted || !c.contains(&en.id))
                            .map(|en| en.id)
                            .collect::<Vec<&'static str>>()
                    }
                    key=|id| *id
                    children=move |id| {
                        entries
                            .get_untracked()
                            .into_iter()
                            .find(|en| en.id == id)
                            .map(|en| (en.inline)())
                            .unwrap_or_else(|| ().into_any())
                    }
                />
            </div>
            <div node_ref=sizer_ref class="tb-sizer" aria-hidden="true">
                {move || {
                    entries
                        .get()
                        .into_iter()
                        .map(|e| (e.sizer)())
                        .collect_view()
                }}
            </div>
        </div>
    }
}

/// Standard overflow-menu row.
///
/// `close_on_click` (default true) dismisses the "…" popover after the
/// action. Set it false for controls that should stay available (zoom ±).
#[component]
pub fn OverflowRow(
    icon: IconName,
    label: &'static str,
    on_click: impl Fn() + 'static,
    done: Callback<()>,
    #[prop(default = true)]
    close_on_click: bool,
) -> impl IntoView {
    view! {
        <button type="button" on:click=move |_| {
            on_click();
            if close_on_click {
                done.run(());
            }
        }
            class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink hover:bg-line">
            <span class="inline-flex w-4 shrink-0 justify-center text-muted"><Icon name=icon size=14 /></span>
            <span>{label}</span>
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::compute_collapsed;

    #[test]
    fn all_fit_returns_empty() {
        let widths = [40.0, 40.0, 40.0];
        let prios = [80, 80, 90];
        assert!(compute_collapsed(&widths, &prios, 200.0, 4.0, 36.0).is_empty());
    }

    #[test]
    fn tight_drops_lowest_priority_first() {
        let widths = [40.0, 40.0, 40.0];
        let prios = [90, 80, 70];
        let dropped = compute_collapsed(&widths, &prios, 125.0, 4.0, 36.0);
        assert!(dropped.contains(&2));
        assert_eq!(dropped.first().copied(), Some(2));
    }

    #[test]
    fn essential_never_dropped() {
        let widths = [40.0, 80.0, 40.0];
        let prios = [70, u32::MAX, 80];
        let dropped = compute_collapsed(&widths, &prios, 50.0, 4.0, 36.0);
        assert!(!dropped.contains(&1));
    }

    #[test]
    fn identical_inputs_are_stable() {
        let widths = [36.0, 36.0, 64.0, 48.0, 40.0];
        let prios = [80, 80, u32::MAX, 90, 70];
        let a = compute_collapsed(&widths, &prios, 140.0, 4.0, 36.0);
        let b = compute_collapsed(&widths, &prios, 140.0, 4.0, 36.0);
        assert_eq!(a, b);
    }

    #[test]
    fn ties_drop_rightmost_first() {
        let widths = [50.0, 50.0];
        let prios = [70, 70];
        let dropped = compute_collapsed(&widths, &prios, 90.0, 4.0, 36.0);
        assert_eq!(dropped, vec![1]);
    }
}
