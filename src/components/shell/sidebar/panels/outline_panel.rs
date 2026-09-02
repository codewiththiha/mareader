//! Document outline (TOC) panel.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;

use pdf_engine::types::OutlineNode;
use app_chrome::hooks::dom::reveal_in_scroll_parent;
use app_chrome::hooks::use_timeout::use_timeout_slot;
use app_chrome::hooks::use_window_event::use_window_event;
use crate::state::ReaderState;
use crate::state::ui::SidebarMode;

fn outline_key(index: usize, node: &OutlineNode) -> String {
    // Index is unique; page + depth keep the key readable when the list
    // rebuilds. Title is left out so a long heading doesn't remount the row.
    format!("{}-{}-{}", index, node.page, node.depth)
}

/// Index of the outline entry the reader is currently inside, if any.
///
/// A TOC entry owns every page from its own up to (but not including) the
/// next entry that starts a later page, so the active entry is the LAST one
/// whose page is at or before the current page.
///
/// Ties matter: several entries can share a page (a chapter and its first
/// section both start on the same page). The later — i.e. deeper — one wins,
/// because that is the more specific description of where the reader is.
///
/// `None` before the first entry's page: a cover or preface belongs to no
/// section, and highlighting chapter 1 there would be a lie.
///
/// Entries arrive flattened in document order, i.e. sorted by page
/// (`loader.ts:flattenOutline`), so one binary search finds the answer:
/// the last index satisfying the predicate is, by construction, the last
/// tie-winner too. A malformed PDF can produce an unsorted tree; the linear
/// fallback keeps the answer correct there, and the debug build asserts the
/// contract so a regression is visible, not silent.
fn active_outline_index(outline: &[OutlineNode], page: u32) -> Option<usize> {
    if !outline.is_sorted_by_key(|node| node.page) {
        return outline.iter().rposition(|node| node.page <= page);
    }
    // `checked_sub` so a page before the first entry yields `None` instead
    // of underflowing the index.
    outline.partition_point(|node| node.page <= page).checked_sub(1)
}

/// Selector for a rendered outline row button by its data attribute. Both the
/// reveal effect and the center-on-tab gesture find their row this way; one
/// definition keeps the attribute name and format from drifting out of sync
/// with the row markup in `outline_view`.
fn outline_row_selector(idx: usize) -> String {
    format!(r#"button[data-outline-index="{idx}"]"#)
}

/// Left padding for an outline row, in px.
///
/// The indent used to be an unbounded `8 + depth * 14`. The sidebar is a fixed
/// 288px (`w-72`), so from about depth 12 the padding consumed the whole row and
/// the title had NO space left: with `truncate` (`overflow:hidden` +
/// `text-overflow:ellipsis`) every deep entry collapsed to a bare "..." — the
/// outline rendering "as dots instead of text". Real-world TOCs nest 5-10 deep
/// (part > chapter > section > subsection > ...), so this is common, and the
/// dots are unclickable-looking and carry no information.
///
/// Two changes: a smaller 12px step, and a hard cap that always leaves room for
/// text. Beyond the cap, depth is still conveyed by the tree order and the
/// hover/active states — losing a few px of indent is far better than losing the
/// title. `INDENT_MAX` keeps at least ~150px for the label.
fn indent_px(depth: u32) -> u32 {
    const BASE: u32 = 8;
    const STEP: u32 = 12;
    const INDENT_MAX: u32 = 120;
    BASE + (depth.saturating_mul(STEP)).min(INDENT_MAX)
}

/// How many reveal attempts (one per frame) before giving up: a genuinely
/// absent row costs 4 frames and no more.
const MAX_REVEAL_ATTEMPTS: u8 = 4;
/// One retry frame.
const REVEAL_RETRY_MS: u64 = 16;

/// One attempt of the reveal retry chain: reveal the row if it is laid out,
/// otherwise schedule the next attempt (one frame later, bounded) into the
/// scope-owned slot. A chain stops the moment its run counter falls behind
/// a newer run, so a stale chain can never scroll onto an old row after a
/// newer reveal has taken over — and the slot's cleanup clears a pending
/// attempt, so nothing ever fires on a detached scroller.
fn reveal_attempt(
    run: RwSignal<u32>,
    my_run: u32,
    slot: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    attempts_left: Rc<Cell<u8>>,
    parent: web_sys::Element,
    idx: usize,
) {
    if run.get_untracked() != my_run {
        return;
    }
    // A row with no height is not laid out yet; keep waiting.
    let row = parent.query_selector(&outline_row_selector(idx)).ok().flatten();
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
        slot.set_value(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::{active_outline_index, indent_px};
    use pdf_engine::types::OutlineNode;

    /// The panel is `w-72` = 288px; `px-3` costs 12px on the right.
    const PANEL_W: u32 = 288;
    const PADDING_RIGHT: u32 = 12;
    /// Enough for a meaningful fragment of a title, not just an ellipsis.
    const MIN_TEXT_W: u32 = 100;

    /// Indent grows with depth, is capped, and — the regression — never eats
    /// so much of the row that the title has no room left (at depth 12+ the
    /// old formula left <= 0px).
    /// The highlight tracks the section the reader is inside: the last entry
    /// at or before the current page, with deeper entries winning ties, and
    /// nothing highlighted before the first entry begins.
    #[test]
    fn active_entry_is_the_section_the_reader_is_in() {
        let node = |page: u32, depth: u32, title: &str| OutlineNode {
            title: title.to_string(),
            page,
            depth,
        };
        let outline = [
            node(3, 0, "Chapter 1"),
            node(3, 1, "1.1 Intro"),   // same page as its parent
            node(9, 1, "1.2 Details"),
            node(20, 0, "Chapter 2"),
        ];
        // Front matter, before the outline starts: nothing is active.
        assert_eq!(active_outline_index(&outline, 1), None);
        assert_eq!(active_outline_index(&outline, 2), None);
        // A tie on page 3 resolves to the deeper (more specific) entry.
        assert_eq!(active_outline_index(&outline, 3), Some(1));
        // Inside a section, before the next one starts.
        assert_eq!(active_outline_index(&outline, 8), Some(1));
        assert_eq!(active_outline_index(&outline, 9), Some(2));
        assert_eq!(active_outline_index(&outline, 19), Some(2));
        // The last entry owns everything to the end of the document.
        assert_eq!(active_outline_index(&outline, 20), Some(3));
        assert_eq!(active_outline_index(&outline, 999), Some(3));
        // No outline at all.
        assert_eq!(active_outline_index(&[], 5), None);
    }

    #[test]
    fn an_unsorted_outline_falls_back_to_the_linear_answer() {
        // A malformed PDF can flatten out of document order; the binary path
        // is skipped and the answer stays the last entry at-or-before.
        let node = |page: u32, depth: u32, title: &str| OutlineNode {
            title: title.to_string(),
            page,
            depth,
        };
        let outline = [node(9, 0, "B"), node(3, 1, "A"), node(20, 0, "C")];
        assert_eq!(active_outline_index(&outline, 5), Some(1));
        assert_eq!(active_outline_index(&outline, 2), None);
        assert_eq!(active_outline_index(&outline, 25), Some(2));
    }

    #[test]
    fn indent_grows_but_always_leaves_room_for_the_title() {
        assert_eq!(indent_px(0), 8);
        assert!(indent_px(0) < indent_px(1));
        assert!(indent_px(1) < indent_px(2));
        assert_eq!(indent_px(1000), indent_px(10), "indent must be capped");
        for depth in 0..64 {
            let text_w = PANEL_W - indent_px(depth) - PADDING_RIGHT;
            assert!(text_w >= MIN_TEXT_W, "depth {depth}: only {text_w}px left for the title");
        }
    }
}

#[component]
pub fn OutlinePanel(
    state: ReaderState,
    /// Which sidebar panel is open (app chrome state passed in explicitly).
    sidebar: RwSignal<SidebarMode>,
) -> impl IntoView {
    let scroller: NodeRef<leptos::html::Div> = NodeRef::new();

    // The active entry is a function of (outline, page): memoizing it keeps
    // a viewport-only scroll from rescanning the outline, and the row list,
    // the reveal effect and the center-on-tab gesture all read one cached
    // value instead of each computing it.
    let active = Memo::new(move |_| {
        let outline = state.document.outline.get();
        active_outline_index(&outline, state.viewer.page.get())
    });

    // Keep the active entry on screen.
    //
    // The panel always rendered from the top, so on a long document the reader
    // had to hunt down the list to find where they were — the highlight was
    // useless until you scrolled to it. This reveals it instead, but only when
    // it is actually off screen (see `reveal_in_scroll_parent`): scrolling on
    // every page change would drag the list out from under someone reading it.
    //
    // Runs on page changes AND when the panel becomes visible, because the rows
    // have no layout while the sidebar is closed and a scroll written then
    // would be silently clamped to zero.
    //
    // The retry chain is ONE scope-owned timer slot plus a run counter: a
    // new run cancels the chain the previous run armed, so a stale reveal can
    // never scroll onto an old row after a newer one, and the slot's cleanup
    // clears a pending attempt (the old implementation held the retry closure
    // in a `Rc<RefCell<Option<Rc<dyn Fn()>>>>` reachable only through a
    // `Weak` downgrade — the same bounded retry without that plumbing).
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
        // Defer past the <For> rebuild.
        //
        // This effect and the row list are driven by the SAME signals, so it
        // can run before the rows for the new page exist — and one frame is
        // not always enough: `aria-current`/`data-outline-index` are re-keyed,
        // so the row for `idx` may still be the OLD node (or missing) on the
        // next frame. Measuring then either finds nothing or scrolls to a
        // stale position, which is why a single-step jump silently did nothing
        // while a multi-step scroll happened to work.
        let parent: web_sys::Element = parent.into();
        let attempts = Rc::new(Cell::new(0u8));
        let handle = set_timeout_with_handle(
            move || reveal_attempt(reveal_run, my_run, reveal_slot, attempts, parent.clone(), idx),
            Duration::from_millis(REVEAL_RETRY_MS),
        )
        .ok();
        reveal_slot.set_value(handle);
    });

    // The deliberate "take me to where I am" gesture: re-clicking the active
    // Outline tab. Unlike the passive reveal above this CENTRES unconditionally
    // — the reader explicitly asked to be moved, so doing nothing because the
    // row is technically one pixel on screen would feel broken.
    Effect::new(move |_| {
        use_window_event(crate::events::REVEAL_ACTIVE_EVENT, move |_: web_sys::Event| {
            if sidebar.get_untracked() != SidebarMode::Outline {
                return;
            }
            let Some(idx) = active.get() else { return };
            let Some(parent) = scroller.get_untracked() else { return };
            let parent: web_sys::Element = parent.into();
            if let Some(row) = parent.query_selector(&outline_row_selector(idx)).ok().flatten() {
                app_chrome::hooks::dom::center_in_scroll_parent(&row, &parent);
            }
        });
    });

    view! {
        <div node_ref=scroller class="flex min-h-0 flex-1 flex-col overflow-y-auto">
            {move || {
                if state.document.outline.get().is_empty() {
                    // The tree resolves lazily (one worker round trip per
                    // chapter destination, after the reader is up): while it
                    // is in flight this is a "not yet", not a "not ever".
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
                                        // Lets the reveal effect locate this
                                        // row without holding a NodeRef per
                                        // entry (the list is rebuilt on every
                                        // page change).
                                        data-outline-index=row_index.to_string()
                                        // `aria-current` is the semantic half of
                                        // the highlight: a screen reader
                                        // announces the active section without
                                        // being able to see the accent bar.
                                        aria-current=move || if is_active { "true" } else { "false" }
                                        // `min-h-7` + `leading-5` is a floor on
                                        // the row box: the engine now
                                        // normalises blank titles to
                                        // "(untitled)", but a row must never be
                                        // able to collapse to a sliver just
                                        // because its text has no height (a
                                        // whitespace/zero-width title used to
                                        // render an 8px row instead of 28px —
                                        // the "barely visible as dots" bug).
                                        class="block min-h-7 w-full truncate border-l-2 px-3 py-1 text-left text-sm leading-5 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                                        // The active row carries the accent on
                                        // its existing left border (already
                                        // reserved as `border-transparent`, so
                                        // nothing shifts when it lights up),
                                        // plus a tinted ground and full-strength
                                        // ink. Inactive rows keep the old muted
                                        // look and hover.
                                        class=("border-accent", move || is_active)
                                        class=("bg-line", move || is_active)
                                        class=("text-ink", move || is_active)
                                        class=("font-medium", move || is_active)
                                        class=("border-transparent", move || !is_active)
                                        class=("text-muted", move || !is_active)
                                        class=("hover:bg-line", move || !is_active)
                                        class=("hover:text-ink", move || !is_active)
                                        style:padding-left=move || format!("{}px", indent_px(depth))
                                        // Jumping does NOT close the sidebar:
                                        // the outline is a map the reader keeps
                                        // open while moving around the
                                        // document, and the highlight only has
                                        // somewhere to show if the panel stays.
                                        on:click=move |_| {
                                            state.viewer.page.set(page);
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
