//! Document outline (TOC) panel.

use leptos::prelude::*;

use pdf_engine::types::OutlineNode;
use crate::state::ReaderState;
use crate::state::ui::SidebarMode;

type RevealHolder = std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>>;

fn outline_key(index: usize, node: &OutlineNode) -> String {
    // Index is unique; page + depth keep the key readable when the list
    // rebuilds. Title is left out so a long heading doesn't remount the row.
    format!("{}-{}-{}", index, node.page, node.depth)
}

/// Index of the outline entry the reader is currently inside, if any.
///
/// A TOC entry owns every page from its own up to (but not including) the next
/// entry that starts a later page, so the active entry is the LAST one whose
/// page is at or before the current page. Entries are already flattened in
/// document order, so a single scan finds it.
///
/// Ties matter: several entries can share a page (a chapter and its first
/// section both start on the same page). The later — i.e. deeper — one wins,
/// because that is the more specific description of where the reader is.
///
/// `None` before the first entry's page: a cover or preface belongs to no
/// section, and highlighting chapter 1 there would be a lie.
fn active_outline_index(outline: &[OutlineNode], page: u32) -> Option<usize> {
    let mut active = None;
    for (i, node) in outline.iter().enumerate() {
        if node.page <= page {
            active = Some(i);
        } else {
            break;
        }
    }
    active
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
    Effect::new(move |_| {
        let page = state.viewer.page.get();
        let showing = sidebar.get() == SidebarMode::Outline;
        let outline = state.document.outline.get();
        let Some(parent) = scroller.get() else { return };
        if !showing || outline.is_empty() {
            return;
        }
        let Some(idx) = active_outline_index(&outline, page) else {
            return;
        };
        let parent: web_sys::Element = parent.into();
        // Defer past the <For> rebuild.
        //
        // This effect and the row list are driven by the SAME signals, so it
        // can run before the rows for the new page exist — and one rAF is not
        // always enough: `aria-current`/`data-outline-index` are re-keyed, so
        // the row for `idx` may still be the OLD node (or missing) on the next
        // frame. Measuring then either finds nothing or scrolls to a stale
        // position, which is why a single-step jump silently did nothing while
        // a multi-step scroll happened to work.
        //
        // Retry across a few frames and stop at the first attempt that finds
        // a laid-out row. Bounded, so a genuinely absent row costs 4 frames
        // and no more.
        let attempt = std::rc::Rc::new(std::cell::Cell::new(0u8));
        let try_reveal: std::rc::Rc<dyn Fn()> = {
            let attempt = attempt.clone();
            let holder: RevealHolder =
                std::rc::Rc::new(std::cell::RefCell::new(None));
            let weak = std::rc::Rc::downgrade(&holder);
            let f: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
                let row = parent
                    .query_selector(&format!("button[data-outline-index=\"{idx}\"]"))
                    .ok()
                    .flatten();
                match row {
                    // A row with no height is not laid out yet; keep waiting.
                    Some(row) if row.get_bounding_client_rect().height() > 0.0 => {
                        crate::components::document::dom_helpers::reveal_in_scroll_parent(&row, &parent, 24.0);
                    }
                    _ => {
                        let n = attempt.get();
                        if n < 4 {
                            attempt.set(n + 1);
                            if let Some(next) =
                                weak.upgrade().and_then(|h| h.borrow().clone())
                            {
                                request_animation_frame(move || next());
                            }
                        }
                    }
                }
            });
            *holder.borrow_mut() = Some(f.clone());
            // Keep the holder alive for as long as the closure can re-arm.
            let keep = holder.clone();
            std::rc::Rc::new(move || {
                let _ = &keep;
                f();
            })
        };
        request_animation_frame(move || try_reveal());
    });

    // The deliberate "take me to where I am" gesture: re-clicking the active
    // Outline tab. Unlike the passive reveal above this CENTRES unconditionally
    // — the reader explicitly asked to be moved, so doing nothing because the
    // row is technically one pixel on screen would feel broken.
    Effect::new(move |_| {
        let handle = window_event_listener(
            leptos::ev::Custom::new("pdfreader:reveal-active"),
            move |_: web_sys::CustomEvent| {
                if sidebar.get_untracked() != SidebarMode::Outline {
                    return;
                }
                let outline = state.document.outline.get_untracked();
                let page = state.viewer.page.get_untracked();
                let Some(idx) = active_outline_index(&outline, page) else { return };
                let Some(parent) = scroller.get_untracked() else { return };
                let parent: web_sys::Element = parent.into();
                if let Some(row) = parent
                    .query_selector(&format!("button[data-outline-index=\"{idx}\"]"))
                    .ok()
                    .flatten()
                {
                    crate::components::document::dom_helpers::center_in_scroll_parent(&row, &parent);
                }
            },
        );
        on_cleanup(move || handle.remove());
    });

    view! {
        <div node_ref=scroller class="flex min-h-0 flex-1 flex-col overflow-y-auto">
            {move || {
                if state.document.outline.get().is_empty() {
                    view! {
                        <div class="flex flex-1 items-center justify-center p-4 text-sm text-muted">No outline</div>
                    }
                    .into_any()
                } else {
                    view! {
                        <For
                            each=move || {
                                let outline = state.document.outline.get();
                                let active = active_outline_index(
                                    &outline,
                                    state.viewer.page.get(),
                                );
                                outline
                                    .into_iter()
                                    .enumerate()
                                    .map(|(i, n)| (i, n, Some(i) == active))
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
