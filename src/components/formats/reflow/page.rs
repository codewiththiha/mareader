//! One reflowable page: an A4 host carrying its blocks as real DOM text.
//!
//! Where a PDF page stretches a raster, this page lays its blocks out for real —
//! which is also why its zoom looks different under the hood. The host is sized
//! `A4 × scale` and the type inside scales by the SAME factor (every size the
//! typography owns is a scale-1 CSS variable on `<html>` multiplied by the host's
//! own `--ts`), so the layout is identical at every scale and the crisp text is
//! never a stretched bitmap. During a zoom the host reflows frame by frame at the
//! live display scale — cheap, because only the mounted pages are in the DOM —
//! while the PAGE CUT stays put: it was computed at scale 1, and uniform scaling
//! provably preserves it, so pagination is never recomputed for a zoom.
//!
//! Paper treatment is the one place text is SIMPLER than PDF: the host is a
//! TRANSPARENT frame over `.reader-bg` (which paints `--tx-paper`), and the
//! type is set in `--tx-ink` — the very tokens the appearance pipeline
//! already paints — so base mode and tint land on exactly the right colours
//! without the `--canvas-filter` or blend-mode machinery the always-light
//! PDF rasters need. No card, no texture rectangle: the texture rides the
//! scroller that is the surface (see `viewer::texture_surface`).
//!
//! The props mirror [`PdfPageCanvas`](crate::components::formats::pdf::PdfPageCanvas)
//! where they mean the same thing — page, scale, host id, class — so the page
//! host can mount either without a format-specific prop in sight. The PDF
//! host's texture prop has no counterpart here: a page of type carries no
//! texture of its own. Two of the raster's props have no meaning for a
//! page of type and are simply absent: no canvas id (there is nothing for the
//! engine to paint into) and no geometry callback (the page cut already knows how
//! tall a page is). The `spine` side is this page's own, and not a mirror of
//! anything: gutter padding is a stylesheet concern, while a raster's gutter is
//! the spread's gap.

use std::sync::Arc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use reflow_core::geometry::{PageGeometry, SpineSide};

use app_chrome::hooks::dom::by_id;

use crate::components::viewer::page_host::block_row_id;
use crate::dom_contract::HOST_REFLOW;
use crate::components::formats::block_render::BlockView;
use super::block_render;
use crate::state::reader::TypographySignal;
use crate::state::reader::ReflowContent;
use crate::state::ReaderState;

/// The page's inline style at the live scale: the page box and its
/// book-layout (or symmetric) paddings, both taken from the geometry the
/// current cut was made with — so the card a reader sees is exactly the card
/// the paginator packed, whatever the margin and column-width dials hold.
/// All paddings are geometry, scaled here; the TYPE inside scales through
/// `--ts` on the content column.
fn page_style(
    page: u32,
    scale: f64,
    book_layout: bool,
    spine: SpineSide,
    geo: PageGeometry,
) -> String {
    let (pad_left, pad_right) =
        geo.pads(book_layout, page.saturating_sub(1) as usize, spine);
    format!(
        "width:{}px;height:{}px;padding:{}px {}px {}px {}px;",
        geo.width * scale,
        geo.height * scale,
        geo.pad_block * scale,
        pad_right * scale,
        geo.pad_block * scale,
        pad_left * scale,
    )
}

/// The content column's inline style: the one multiplier every typography knob
/// resolves through. The knobs themselves live as scale-1 custom properties on
/// `<html>` (painted by the typography effect); em-valued ones ride the scaled
/// font size on their own, px-valued ones multiply `--ts` explicitly in the
/// stylesheet.
pub(crate) fn content_style(scale: f64) -> String {
    format!("--ts:{};", scale)
}

#[component]
pub fn ReflowPage(
    /// 1-based page number this host renders.
    page: u32,
    state: ReaderState,
    /// The live display scale, from the page host (the same signal a PDF page is
    /// stretched by).
    scale: ReadSignal<f64>,
    /// The host element's id. Supplied by the page host so a page of type and a
    /// page of pixels answer to the SAME id in the same slot — which is what
    /// lets anything that addresses the current page from outside the component
    /// tree (the floating chapter label, a selection anchor) stop caring which
    /// pipeline produced it.
    #[prop(into)]
    host_id: String,
    /// Extra classes (the cross-axis `mx-auto` that centres a page and degrades
    /// to start-alignment on overflow, same as the PDF host).
    #[prop(default = String::new(), into)]
    class: String,
    /// Where this page sits relative to the book spine. The spread fixes its two
    /// pages to the two sides; everywhere else the parity alternates on its own.
    #[prop(default = SpineSide::Auto)]
    spine: SpineSide,
) -> impl IntoView {
    let typography =
        use_context::<TypographySignal>().expect("TypographySignal must be provided by app bootstrap");
    let book_layout = Memo::new(move |_| typography.get().book_layout);
    // One class, always: the host is a transparent frame, never a textured
    // card — the texture lives on the scroller (see `viewer::texture_surface`).
    let host_class =
        move || if class.is_empty() { "tx-page".to_string() } else { format!("tx-page {class}") };

    let reflow = state.document.content.reflow;
    // The cut's own geometry, read tracked: the measurement pipeline
    // re-publishes it whenever the margin or column-width dial moves, and the
    // card must follow — a wider dialled column is a wider card wrapping it,
    // with the pads to match, or the type would outgrow the box it was
    // measured in.
    let geometry = move || reflow.geometry.get();
    let render = block_render(state);
    // The host id is the element's, the gloss resolver's and the coordinate
    // origin its strokes are positioned against; each consumer needs its own.
    let gloss_host_id = host_id.clone();
    let gloss_layer_host = host_id.clone();
    // The block range is read TRACKED: a re-cut publishes a new range for the
    // same page number, and the host must re-render it.
    let range = move || page_range(reflow, page);

    // The blocks this host renders are its own measurement source: their
    // rendered heights, divided back to scale 1, feed the shared store the
    // page cut follows (`crate::effects::reader::reflow_measure`), so the
    // breaks CONVERGE on what has actually been read instead of holding the
    // open-time estimate forever. The page strip's own sizes never hear
    // about these numbers — a strip page is the sheet (`PAGE_HEIGHT` per
    // cut), not the sum of its blocks — only the store does, and through it
    // the next re-cut. Read at the display scale, reported at scale 1, and
    // silent while a zoom transaction owns the geometry.
    {
        Effect::new(move |_| {
            let (start, end) = range();
            let scale = scale.get();
            let _typography = typography.get();
            let _ = state.viewer.page_margin.get();
            let _ = state.viewer.column_width_pct.get();
            if start == end {
                return;
            }
            request_animation_frame(move || {
                if state.viewer.zooming_now() {
                    return;
                }
                let doc_id = reflow
                    .blocks
                    .with_untracked(|blocks| Arc::as_ptr(blocks) as usize);
                let mut batch: Vec<(usize, f64)> = Vec::new();
                for index in start..end {
                    let Some(row) = by_id(&block_row_id(index)) else {
                        continue;
                    };
                    let Ok(el) = row.dyn_into::<web_sys::HtmlElement>() else {
                        continue;
                    };
                    let height = el.offset_height() as f64;
                    if height > 0.0 && scale > 0.0 {
                        batch.push((index, height / scale));
                    }
                }
                crate::effects::reader::reflow_measure::ingest(doc_id, scale, &batch);
            });
        });
    }

    view! {
        <div
            id=gloss_host_id
            class=host_class
            style=move || page_style(page, scale.get(), book_layout.get(), spine, geometry())
            // The two facts the AI feature reads off a host instead of asking
            // which pipeline painted it: what family this is, and which page.
            // The selection tracker finds its host through the first, and the
            // anchor resolvers address pages through the second (see
            // `crate::components::ai::reflow_anchor`).
            data-reader-host=HOST_REFLOW
            data-host-page=page
        >
            <div class="tx-content" lang="en" style=move || content_style(scale.get())>
                <For
                    each=move || {
                        let doc_id = reflow.document_id();
                        let (start, end) = range();
                        (start..end).map(|index| (doc_id, index)).collect::<Vec<(usize, usize)>>()
                    }
                    key=|(doc_id, index): &(usize, usize)| (*doc_id, *index)
                    children=move |(_, index): (usize, usize)| {
                        match reflow.block_at(index) {
                            Some(block) => {
                                view! { <BlockView state=state block=block render=render index=index /> }
                                    .into_any()
                            }
                            // A re-cut can briefly hold a window from the
                            // outgoing pagination; an out-of-range index renders
                            // nothing rather than panicking.
                            None => ().into_any(),
                        }
                    }
                />
            </div>
            // Persisted gloss highlights, in the same place a PDF page keeps
            // them: inside the host, so a remount repaints them and a mark
            // survives the virtualizer, a zoom and a view-mode flip. The
            // resolver is what makes them land on the right words — a
            // reflowable mark is a block and a character range, re-projected
            // onto whatever the browser just laid out.
            <crate::components::formats::reflow::ReflowGlossLayer
                state=state
                page=page
                host_id=gloss_layer_host
            />
        </div>
    }
}

/// The block range of `page`, read tracked (a re-cut republishes it). A page the
/// cut does not have — an unpaginated document, a page beyond the last — reads
/// as an empty range, so the host paints paper with no type rather than the whole
/// file.
fn page_range(reflow: ReflowContent, page: u32) -> (usize, usize) {
    reflow.cuts.with(|cuts| {
        cuts.get(page.saturating_sub(1) as usize)
            .map(|cut| (cut.start, cut.end()))
            .unwrap_or((0, 0))
    })
}
