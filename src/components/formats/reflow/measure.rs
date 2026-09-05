//! The measure column: the DOM's honest answer to "how tall is each block".
//!
//! The open flow seeds pagination from a pure estimate (character counts),
//! which is good enough to lay the book out the instant it opens but is not
//! the truth — real fonts, real kerning, real Markdown constructs all move
//! the numbers. This column renders every block ONCE, offscreen, at scale 1
//! and exactly the typography the pages use, reads the rendered heights,
//! and republishes the page cut from them. After that first pass the cut is
//! measurement-true, and it only moves again when the typography (or the
//! book-layout toggle that changes the column width) changes.
//!
//! Zoom deliberately never reaches this column: heights are scale-1 truths,
//! and a uniform scale provably preserves the cut (see `components::formats::reflow::page`).

use std::sync::Arc;

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use reflow_core::geometry::geometry;
use reflow_core::pager::paginate;
use reflow_core::typography::TextSettings;

use crate::components::formats::block_render::BlockView;
use super::block_render;
use super::page::content_style;
use crate::state::reader::TypographySignal;
use crate::state::AppState;

/// The offscreen box, written inline so the twin hides itself from its very
/// first frame even if `styles.css` has not landed yet (see the view below).
/// `styles/text.css` carries the same declarations on `.tx-measure` for parity.
const HIDDEN_TWIN: &str =
    "position:fixed;left:-100000px;top:0;visibility:hidden;pointer-events:none;z-index:-1;";

/// How many frames the measure pass re-checks itself before giving up on a
/// layout the browser has not committed yet (a column that measures 0 tall,
/// children not attached yet). Each retry re-reads the DOM; nothing is
/// guessed.
const MEASURE_SETTLE_FRAMES: u32 = 4;

#[component]
pub fn ReflowMeasureColumn(app: AppState) -> impl IntoView {
    let typography =
        use_context::<TypographySignal>().expect("TypographySignal must be provided by app bootstrap");
    let container: NodeRef<html::Div> = NodeRef::new();

    // The measure pass. Tracked reads: the document (a new file remeasures),
    // the typography (any knob moves the heights), and the remeasure epoch
    // (the explicit "again" lever). The pass itself runs on the next frame,
    // so the column's own re-render for the new inputs has already landed.
    Effect::new(move |_| {
        // Tracked, and read unconditionally: an empty block list is the "no
        // document" answer every other surface gives, and a `return` before the
        // typography read below would silently unsubscribe the column from the
        // very knobs it exists to follow.
        let count = app.reader.document.content.reflow.blocks.with(|blocks| blocks.len());
        if count == 0 {
            return;
        }
        let t = typography.get();
        let _epoch = app.reader.document.content.reflow.remeasure.get();
        let col = container;
        request_animation_frame(move || {
            measure_pass(app, col, count, &t, MEASURE_SETTLE_FRAMES);
        });
    });

    // The column itself: fixed offscreen, exactly the page's content width,
    // wearing exactly the page's typography (at scale 1). The `tx-content`
    // class is the load-bearing half of that promise: every typographic
    // rule (font, size, line height, spacing) resolves on that class, so a
    // measure column without it measures the browser's fallback face and
    // paginates against fiction. `visibility: hidden` keeps layout honest —
    // `display:none` would not lay out at all.
    //
    // The offscreen box is ALSO written inline, and that is not duplication
    // for its own sake. `styles.css` is compiled by a Trunk `pre_build` hook,
    // so the module can boot before (or between) stylesheet swaps; a column
    // whose only hiding rule lives in a sheet that has not landed yet is a
    // full-width, fully painted copy of the document stacked at the top of
    // `<main>` — the dark ghost that shimmered behind a freshly opened text
    // file. An inline style cannot be late, so the twin is invisible from its
    // very first frame no matter what the cascade is doing. `text.css` keeps
    // the same declarations for parity.
    view! {
        <div
            node_ref=container
            class="tx-measure tx-content"
            aria-hidden="true"
            lang="en"
            style:width=move || format!("{}px", geometry(typography.get().book_layout).content_width)
            style=move || format!("{}{}", HIDDEN_TWIN, content_style(1.0))
        >
            <For
                each=move || {
                    app.reader.document.content.reflow.blocks.with(|blocks| {
                        // The Arc pointer IS the document's identity:
                        // keying on it means a new document remounts
                        // every block instead of reusing index keys the
                        // outgoing file already occupied.
                        let doc_id = Arc::as_ptr(blocks) as usize;
                        blocks
                            .iter()
                            .enumerate()
                            .map(|(index, block)| (doc_id, index, block.clone()))
                            .collect::<Vec<_>>()
                    })
                }
                key=|(doc_id, index, _): &(usize, usize, reflow_core::block::TextBlock)| {
                    (*doc_id, *index)
                }
                children=move |(_, _, block): (usize, usize, reflow_core::block::TextBlock)| {
                    view! { <BlockView block=block render=block_render(app.reader) state=app.reader /> }
                }
            />
        </div>
    }
}

/// One measurement attempt: read every block's rendered height and, if the
/// cut those heights produce differs from the live one, publish it.
fn measure_pass(
    app: AppState,
    container: NodeRef<html::Div>,
    count: usize,
    typography: &TextSettings,
    frames_left: u32,
) {
    let Some(col) = container.get() else {
        retry(app, container, count, typography, frames_left);
        return;
    };
    let children = col.children();
    // The column re-renders in the same flush as the inputs that triggered
    // the pass, but a browser that has not committed it yet shows the OLD
    // children (or none). Retry until it does.
    if children.length() as usize != count {
        retry(app, container, count, typography, frames_left);
        return;
    }
    let mut heights = Vec::with_capacity(count);
    for index in 0..count {
        let Some(child) = children.item(index as u32) else {
            retry(app, container, count, typography, frames_left);
            return;
        };
        let Ok(el) = child.dyn_into::<web_sys::HtmlElement>() else {
            retry(app, container, count, typography, frames_left);
            return;
        };
        heights.push(el.offset_height() as f64);
    }

    let geo = geometry(typography.book_layout);
    let new_cuts = paginate(&heights, geo.content_height);
    let changed = app
        .reader
        .document
        .content
        .reflow
        .cuts
        .with_untracked(|cuts| cuts.as_ref() != &new_cuts)
        || app
            .reader
            .document
            .content
            .reflow
            .geometry
            .with_untracked(|g| *g != geo);
    if !changed {
        // Same cut: keep the measured heights as the standing truth (the
        // next comparison should run against reality, not the estimate),
        // and leave the reader's position entirely alone.
        //
        // But only publish them when they actually moved. `heights` feeds the
        // stream's epoch and both re-measure effects, so an unconditional
        // `set` on every pass wakes the whole reflowable side of the reader
        // to hand it the numbers it already had — and the settle pass after
        // first paint runs exactly when the reader is busiest. A re-measure
        // that changed nothing now changes nothing.
        let same = app
            .reader
            .document
            .content
            .reflow
            .heights
            .with_untracked(|current| current.as_ref() == &heights);
        if !same {
            app.reader.document.content.reflow.heights.set(Arc::new(heights));
        }
        return;
    }

    // A real re-cut: republish the cut, tell the document's shared page
    // machinery what it now holds, and hold the reader on the block they were
    // reading — the cut answers the page that block now sits on.
    let cut = app.reader.document.content.reflow.apply_heights(app, heights, geo);
    app.reader.document.publish_cut(&cut);
    app.reader.viewer.page.set(cut.page);
}

/// Schedule another attempt, one frame later, until the budget runs out.
fn retry(
    app: AppState,
    container: NodeRef<html::Div>,
    count: usize,
    typography: &TextSettings,
    frames_left: u32,
) {
    if frames_left == 0 {
        return;
    }
    let t = typography.clone();
    request_animation_frame(move || {
        measure_pass(app, container, count, &t, frames_left - 1);
    });
}
