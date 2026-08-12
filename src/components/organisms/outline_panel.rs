//! Document outline (TOC) panel. OWNED BY branch C (panels/sidebar).

use leptos::prelude::*;

use crate::core::document::OutlineNode;
use crate::core::state::{AppState, SidebarMode};

fn outline_key(node: &OutlineNode) -> String {
    format!("{}-{}-{}", node.page, node.depth, node.title)
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
    use super::indent_px;

    /// The panel is `w-72` = 288px; `px-3` costs 12px on the right.
    const PANEL_W: u32 = 288;
    const PADDING_RIGHT: u32 = 12;
    /// Enough for a meaningful fragment of a title, not just an ellipsis.
    const MIN_TEXT_W: u32 = 100;

    /// Indent grows with depth, is capped, and — the regression — never eats
    /// so much of the row that the title has no room left (at depth 12+ the
    /// old formula left <= 0px).
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
pub fn OutlinePanel(state: AppState) -> impl IntoView {
    view! {
        <div class="flex min-h-0 flex-1 flex-col overflow-y-auto">
            {move || {
                if state.doc.outline.get().is_empty() {
                    view! {
                        <div class="flex flex-1 items-center justify-center p-4 text-sm text-muted">No outline</div>
                    }
                    .into_any()
                } else {
                    view! {
                        <For
                            each=move || state.doc.outline.get()
                            key=outline_key
                            children=move |node: OutlineNode| {
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
                                        // `min-h-7` + `leading-5` is a floor on
                                        // the row box: the engine now
                                        // normalises blank titles to
                                        // "(untitled)", but a row must never be
                                        // able to collapse to a sliver just
                                        // because its text has no height (a
                                        // whitespace/zero-width title used to
                                        // render an 8px row instead of 28px —
                                        // the "barely visible as dots" bug).
                                        class="block min-h-7 w-full truncate border-l-2 border-transparent px-3 py-1 text-left text-sm leading-5 text-muted hover:bg-line hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                                        style:padding-left=move || format!("{}px", indent_px(depth))
                                        on:click=move |_| {
                                            state.viewer.page.set(page);
                                            state.sidebar.set(SidebarMode::None);
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
