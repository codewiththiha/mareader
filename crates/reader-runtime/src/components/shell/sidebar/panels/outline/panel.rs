//! Document outline (TOC) panel.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;

use crate::state::ReaderState;
use app_chrome::hooks::dom::reveal_in_scroll_parent;
use app_chrome::hooks::use_timeout::use_timeout_slot;
use app_chrome::hooks::use_window_event::use_window_event;
use app_state::SidebarMode;
use reader_core::outline::{OutlineNode, active_entry};

fn outline_key(index: usize, node: &OutlineNode) -> String {
    // Index is unique; page + depth keep the key readable on rebuild.
    format!("{}-{}-{}", index, node.page, node.depth)
}

/// Selector for an outline row button by its data attribute, shared by
/// both finders.
fn outline_row_selector(idx: usize) -> String {
    format!(r#"button[data-outline-index="{idx}"]"#)
}

/// The active entry, honouring the reader's click while it still
/// answers.
fn active_with_click(
    outline: &[OutlineNode],
    page: u32,
    clicked: Option<(usize, u32)>,
) -> Option<usize> {
    match clicked {
        Some((index, at)) if at == page && index < outline.len() => Some(index),
        _ => active_entry(outline, page),
    }
}

/// Indent for an outline row, in px: a 12px step, hard-capped.
fn indent_px(depth: u32) -> u32 {
    const BASE: u32 = 8;
    const STEP: u32 = 12;
    const INDENT_MAX: u32 = 120;
    BASE + (depth.saturating_mul(STEP)).min(INDENT_MAX)
}

/// Reveal attempts, one per frame, before giving up.
const MAX_REVEAL_ATTEMPTS: u8 = 4;
/// One retry frame.
const REVEAL_RETRY_MS: u64 = 16;

/// One attempt of the reveal retry chain: reveal, or schedule the
/// next bounded attempt.
fn reveal_attempt(
    run: RwSignal<u32>,
    my_run: u32,
    slot: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    attempts_left: Rc<Cell<u8>>,
    parent: web_sys::Element,
    idx: usize,
) {
    // The chain is a QUEUED callback: the slot ends it when the scope
    // is gone.
    if slot.try_get_value().is_none() {
        return;
    }
    let Some(run_now) = run.try_get_untracked() else {
        return;
    };
    if run_now != my_run {
        return;
    }
    // A row with no height is not laid out yet; keep waiting.
    let row = parent
        .query_selector(&outline_row_selector(idx))
        .ok()
        .flatten();
    if let Some(row) = row
        && row.get_bounding_client_rect().height() > 0.0
    {
        reveal_in_scroll_parent(&row, &parent, 24.0);
        return;
    }
    let n = attempts_left.get();
    if n < MAX_REVEAL_ATTEMPTS {
        attempts_left.set(n + 1);
        let handle = set_timeout_with_handle(
            move || reveal_attempt(run, my_run, slot, attempts_left, parent.clone(), idx),
            Duration::from_millis(REVEAL_RETRY_MS),
        )
        .ok();
        let _ = slot.try_set_value(handle);
    }
}

#[component]
pub fn OutlinePanel(
    state: ReaderState,
    /// Which sidebar panel is open (app chrome state passed in explicitly).
    sidebar: RwSignal<SidebarMode>,
) -> impl IntoView {
    let scroller: NodeRef<leptos::html::Div> = NodeRef::new();

    // The entry the rows mark: the reader's click, else the page's.
    let clicked = RwSignal::new(None::<(usize, u32)>);
    // Memoized: one cached answer for the rows, reveal and centre.
    let active = Memo::new(move |_| {
        let outline = state.document.outline.get();
        let page = state.viewer.page.get();
        active_with_click(&outline, page, clicked.get())
    });

    // Keep the active entry on screen, but only when it is off screen.
    let reveal_slot = use_timeout_slot();
    let reveal_run = RwSignal::new(0u32);
    Effect::new(move |_| {
        let showing = sidebar.get() == SidebarMode::Outline;
        let outline = state.document.outline.get();
        let Some(parent) = scroller.get() else {
            return;
        };
        if !showing || outline.is_empty() {
            return;
        }
        let Some(idx) = active.get() else {
            return;
        };
        // A newer run supersedes any retry chain still in flight.
        if let Some(h) = reveal_slot.try_get_value().flatten() {
            h.clear();
        }
        reveal_run.update(|run| *run += 1);
        let my_run = reveal_run.get();
        // Defer past the <For> rebuild: the row for `idx` may still be the
        // old node.
        let parent: web_sys::Element = parent.into();
        let attempts = Rc::new(Cell::new(0u8));
        let handle = set_timeout_with_handle(
            move || {
                reveal_attempt(
                    reveal_run,
                    my_run,
                    reveal_slot,
                    attempts,
                    parent.clone(),
                    idx,
                )
            },
            Duration::from_millis(REVEAL_RETRY_MS),
        )
        .ok();
        let _ = reveal_slot.try_set_value(handle);
    });

    // The deliberate "take me to where I am" gesture: it CENTRES
    // unconditionally.
    Effect::new(move |_| {
        use_window_event(
            app_ui::events::REVEAL_ACTIVE_EVENT,
            move |_: web_sys::Event| {
                if sidebar.get_untracked() != SidebarMode::Outline {
                    return;
                }
                let Some(idx) = active.get() else { return };
                let Some(parent) = scroller.get_untracked() else {
                    return;
                };
                let parent: web_sys::Element = parent.into();
                if let Some(row) = parent
                    .query_selector(&outline_row_selector(idx))
                    .ok()
                    .flatten()
                {
                    app_chrome::hooks::dom::center_in_scroll_parent(&row, &parent);
                }
            },
        );
    });

    view! {
        <div node_ref=scroller class="flex min-h-0 flex-1 flex-col overflow-y-auto">
            {move || {
                if state.document.outline.get().is_empty() {
                    // The tree resolves lazily: this is a "not yet".
                    let pending = state.document.outline_pending.get();
                    view! {
                        <div class="flex flex-1 items-center justify-center p-4 text-sm text-muted">
                            {if pending { "Resolving chapters…" } else { "No outline" }}
                        </div>
                    }
                    .into_any()
                } else {
                    view! {
                        <For
                            each=move || {
                                let outline = state.document.outline.get();
                                let active = active.get();
                                outline
                                    .iter()
                                    .enumerate()
                                    .map(|(i, n)| (i, n.clone(), Some(i) == active))
                                    .collect::<Vec<_>>()
                            }
                            key=|(i, node, is_active): &(usize, OutlineNode, bool)| {
                                format!("{}-{}", outline_key(*i, node), is_active)
                            }
                            children=move |(row_index, node, is_active): (usize, OutlineNode, bool)| {
                                let page = node.page;
                                let depth = node.depth;
                                let title = node.title.clone();
                                // Truncation is still possible for a genuinely
                                // long title, so expose the full text natively.
                                let tooltip = title.clone();
                                view! {
                                    <button
                                        type="button"
                                        title=tooltip
                                        // Lets the reveal find this row.
                                        data-outline-index=row_index.to_string()
                                        // The highlight's semantic half.
                                        aria-current=move || if is_active { "true" } else { "false" }
                                        // A floor on the row box, so a
                                        // title with no height cannot
                                        // collapse it.
                                        class="block min-h-7 w-full truncate border-l-2 px-3 py-1 text-left text-sm leading-5 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                                        // The active row: accent
                                        // border, tinted ground,
                                        // full-strength ink.
                                        class=("border-accent", move || is_active)
                                        class=("bg-line", move || is_active)
                                        class=("text-ink", move || is_active)
                                        class=("font-medium", move || is_active)
                                        class=("border-transparent", move || !is_active)
                                        class=("text-muted", move || !is_active)
                                        class=("hover:bg-line", move || !is_active)
                                        class=("hover:text-ink", move || !is_active)
                                        style:padding-left=move || format!("{}px", indent_px(depth))
                                        // Jumping keeps the sidebar open.
                                        on:click=move |_| {
                                            // The block jump moves a text
                                            // document; the page write
                                            // carries the counter.
                                            state.viewer.page.set(page);
                                            clicked.set(Some((row_index, page)));
                                            if state.reflowable_now() {
                                                state.viewer.ask_outline_jump(row_index as u32);
                                            }
                                        }
                                    >
                                        {title}
                                    </button>
                                }
                            }
                        />
                    }
                    .into_any()
                }
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::{active_with_click, indent_px};
    use reader_core::outline::{OutlineNode, active_entry};

    /// The panel is `w-72` = 288px; `px-3` costs 12px on the right.
    const PANEL_W: u32 = 288;
    const PADDING_RIGHT: u32 = 12;
    /// Enough for a meaningful fragment of a title, not just an ellipsis.
    const MIN_TEXT_W: u32 = 100;

    /// Indent grows with depth, capped so the title always has room.
    #[test]
    fn indent_grows_but_always_leaves_room_for_the_title() {
        assert_eq!(indent_px(0), 8);
        assert!(indent_px(0) < indent_px(1));
        assert!(indent_px(1) < indent_px(2));
        assert_eq!(indent_px(1000), indent_px(10), "indent must be capped");
        for depth in 0..64 {
            let text_w = PANEL_W - indent_px(depth) - PADDING_RIGHT;
            assert!(
                text_w >= MIN_TEXT_W,
                "depth {depth}: only {text_w}px left for the title"
            );
        }
    }

    /// The click's entry answers ahead of the rule's later entry on
    /// that page.
    #[test]
    fn a_clicked_entry_holds_the_highlight_for_its_own_page() {
        let node = |title: &str, page: u32, depth: u32| OutlineNode::new(title, page, depth);
        let outline = [
            node("Chapter 1", 3, 0),
            node("1.1 Intro", 3, 1),
            node("1.2 Details", 9, 1),
        ];
        // The page rule lights the later entry of the two sharing page 3...
        assert_eq!(active_entry(&outline, 3), Some(1));
        // ...and the click's own entry wins while its page is the one shown.
        assert_eq!(active_with_click(&outline, 3, Some((0, 3))), Some(0));
        // The reader moving on hands the highlight back to the rule.
        assert_eq!(active_with_click(&outline, 9, Some((0, 3))), Some(2));
        // A click this outline cannot answer is not an answer at all.
        assert_eq!(active_with_click(&outline, 3, Some((7, 3))), Some(1));
    }
}
