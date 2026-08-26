//! Collision-aware toolbar group. Items that don't fit move into a
//! conditional "…" overflow popover; with room to spare the button vanishes.
use std::sync::Arc;
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::primitives::hooks::use_resize_observer::observe_elements;
use crate::components::primitives::icon::IconName;
use crate::components::primitives::icon_button::IconButton;
use crate::components::primitives::hooks::dom::{
    by_id, TOOLBAR_LEADING_ID, TOOLBAR_ROW_ID, TOOLBAR_TRAILING_ID,
};
use crate::components::app_shell::toolbar_popover::MenuPopover;
use super::toolbar_overflow::compute_collapsed;

pub const TB_GAP: f64 = 4.0;          // gap-1
pub const TB_OVERFLOW_W: f64 = 36.0;  // h-9 w-9
pub const TB_RIGHT_RESERVE: f64 = crate::components::app_shell::constants::PIN_RESERVE;
pub const TB_TITLE_RESERVE: f64 = crate::components::app_shell::constants::MIN_DOC_TITLE_WIDTH;

/// One control that can live in the bar or in the overflow menu.
///
/// The three views are `Arc<dyn Fn ... Send + Sync>` closures because the
/// entries live in a `RwSignal<Vec<ToolbarItem>>` — Leptos' default signal
/// storage requires `Send + Sync + 'static` contents (single-threaded WASM
/// today, but the storage contract is what it is), and the sizer/inline pair
/// must share ONE closure (`pair`) so the measured width can never drift
/// from what the bar renders. `pair` exists so callers never wire those two
/// by hand.
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
            let Some(row) = by_id(TOOLBAR_ROW_ID) else { return };
            let list = entries.get_untracked();
            let kids = sizer.children();
            let widths: Vec<f64> = (0..list.len() as u32)
                .map(|i| kids.item(i).map(|e| e.get_bounding_client_rect().width()).unwrap_or(0.0))
                .collect();
            let priorities: Vec<u32> = list.iter().map(|e| e.priority).collect();
            let rr = row.get_bounding_client_rect();
            let left_end = by_id(TOOLBAR_LEADING_ID)
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

    // The shared resize-observer primitive owns the Closure/StoredValue
    // dance; this effect collects the live element set (re-running as the
    // sizer / row / slot nodes mount) and hands it over once.
    Effect::new(move |_| {
        let Some(sizer) = sizer_ref.get() else { return };
        let mut els = vec![sizer.unchecked_into::<web_sys::Element>()];
        if let Some(row) = by_id(TOOLBAR_ROW_ID) {
            els.push(row);
        }
        for id in [TOOLBAR_LEADING_ID, TOOLBAR_TRAILING_ID] {
            if let Some(el) = by_id(id) {
                els.push(el);
            }
        }
        observe_elements(els, move |_| recalc());
    });
    Effect::new(move |_| {
        let _ = refresh.get();
        recalc();
    });

    view! {
        <div class="relative shrink-0">
            <div id=TOOLBAR_TRAILING_ID data-tauri-drag-region="true" class="flex shrink-0 items-center gap-1">
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
                        <MenuPopover open=open anchor=overflow_ref width=224 coordinate_space="toolbar-row" class="p-1".to_string()>
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
                        </MenuPopover>
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
