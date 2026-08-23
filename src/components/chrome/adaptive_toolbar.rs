//! Collision-aware toolbar group. Items that don't fit move into a
//! conditional "…" overflow popover; with room to spare the button vanishes.
use std::sync::Arc;
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;
use crate::components::primitives::icon::IconName;
use crate::components::primitives::icon_button::IconButton;
use crate::components::document::dom_helpers::by_id;
use crate::components::primitives::popover::Popover;
use super::toolbar_layout::compute_collapsed;

pub const TB_GAP: f64 = 4.0;          // gap-1
pub const TB_OVERFLOW_W: f64 = 36.0;  // h-9 w-9
pub const TB_RIGHT_RESERVE: f64 = crate::components::chrome::metrics::PIN_RESERVE;
pub const TB_TITLE_RESERVE: f64 = crate::components::chrome::metrics::MIN_DOC_TITLE_WIDTH;

/// One control that can live in the bar or in the overflow menu.
#[derive(Clone)]
pub struct ToolbarItem {
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

impl ToolbarItem {
    /// Bar and sizer share one view. Overflow supplies its own row.
    pub fn pair(
        id: &'static str,
        priority: u32,
        view: impl Fn() -> AnyView + Send + Sync + 'static,
        collapsed: impl Fn(Callback<()>) -> AnyView + Send + Sync + 'static,
    ) -> Self {
        let v = Arc::new(view);
        Self {
            id,
            priority,
            keep_mounted: false,
            inline: v.clone(),
            sizer: v,
            collapsed: Arc::new(collapsed),
        }
    }
}

#[component]
pub fn AdaptiveToolbar(
    /// True while a document is open: the title reserve applies then, so
    /// the bar always leaves the document name room.
    ready: Signal<bool>,
    /// Bumped by the caller whenever chrome state that affects the bar's
    /// geometry changes (sidebar open/close, document identity, page
    /// count) — the group re-measures in response.
    refresh: Signal<u64>,
    entries: Vec<ToolbarItem>,
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
            let title_reserve = if ready.get_untracked() {
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
        let _ = refresh.get();
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
                        <IconButton
                            icon=IconName::More
                            title="More tools"
                            on_click=move || open.set(!open.get())
                        />
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

#[cfg(test)]
mod tests {
    use super::ToolbarItem;
    use leptos::prelude::IntoAny;

    #[test]
    fn pair_shares_inline_and_sizer() {
        let item = ToolbarItem::pair("demo", 50, || ().into_any(), |_| ().into_any());
        assert_eq!(item.id, "demo");
        assert!(!item.keep_mounted);
        assert_eq!(item.priority, 50);
    }
}
