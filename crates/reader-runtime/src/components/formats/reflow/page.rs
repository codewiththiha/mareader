//! One reflowable page: an A4 host carrying its blocks as real DOM text.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use reflow_core::geometry::{PageGeometry, SpineSide};

use super::block_render;
use crate::components::formats::block_render::BlockView;
use crate::components::viewer::page_host::block_row_id;
use crate::state::ReaderState;
use crate::state::ReflowContent;
use crate::state::TypographySignal;
use app_state::dom_contract::HOST_REFLOW;

/// The page's inline style at the live scale, from the cut's
/// geometry.
fn page_style(
    page: u32,
    scale: f64,
    book_layout: bool,
    spine: SpineSide,
    geo: PageGeometry,
) -> String {
    let (pad_left, pad_right) = geo.pads(book_layout, page.saturating_sub(1) as usize, spine);
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

/// The content column's inline style: the one multiplier typography
/// resolves through.
pub(crate) fn content_style(scale: f64) -> String {
    format!("--ts:{};", scale)
}

#[component]
pub fn ReflowPage(
    /// 1-based page number this host renders.
    page: u32,
    state: ReaderState,
    /// The live display scale, from the page host.
    scale: ReadSignal<f64>,
    /// The host element's id, the SAME for a page of type or pixels.
    #[prop(into)]
    host_id: String,
    /// Extra classes (the cross-axis `mx-auto` that centres a page).
    #[prop(default = String::new(), into)]
    class: String,
    /// Where this page sits relative to the book spine.
    #[prop(default = SpineSide::Auto)]
    spine: SpineSide,
) -> impl IntoView {
    let typography = use_context::<TypographySignal>()
        .expect("TypographySignal must be provided by app bootstrap");
    let book_layout = Memo::new(move |_| typography.get().book_layout);
    // One class, always: the texture lives on the scroller.
    let host_class = move || {
        if class.is_empty() {
            "tx-page".to_string()
        } else {
            format!("tx-page {class}")
        }
    };

    let reflow = state.document.content.reflow;
    // The cut's geometry, read tracked: a dial move re-publishes it and
    // the card follows.
    let geometry = move || reflow.geometry.get();
    let render = block_render(state);
    // The host id is shared by the element, the gloss layer and the
    // strokes.
    let gloss_host_id = host_id.clone();
    let gloss_layer_host = host_id.clone();
    // Read TRACKED: a re-cut publishes a new range for the same page.
    let range = move || page_range(reflow, page);

    // The blocks this host renders feed the measurement store.
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
                // `None` is the disposed reader: no reflow left to measure.
                if state.viewer.try_zooming_now() != Some(false) {
                    return;
                }
                // No session, nothing to report.
                let Some(session) = state.pane.reflow_session() else {
                    return;
                };
                let mut batch: Vec<(usize, f64)> = Vec::new();
                for index in start..end {
                    let Some(row) = state.dom.by_id(&block_row_id(index)) else {
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
                state.measure.ingest(session, scale, &batch);
            });
        });
    }

    view! {
        <div
            id=gloss_host_id
            class=host_class
            style=move || page_style(page, scale.get(), book_layout.get(), spine, geometry())
            // What the AI feature reads off a host: family and page.
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
                            // Out-of-range renders nothing, not a panic.
                            None => ().into_any(),
                        }
                    }
                />
            </div>
            // Persisted gloss highlights live inside the host, as for a PDF.
            <crate::components::formats::reflow::ReflowGlossLayer
                state=state
                page=page
                host_id=gloss_layer_host
            />
        </div>
    }
}

/// The block range of `page`, read tracked; a page the cut lacks reads
/// empty.
fn page_range(reflow: ReflowContent, page: u32) -> (usize, usize) {
    reflow.cuts.with(|cuts| {
        cuts.get(page.saturating_sub(1) as usize)
            .map(|cut| (cut.start, cut.end()))
            .unwrap_or((0, 0))
    })
}
