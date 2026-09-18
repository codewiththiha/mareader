//! The placeholder tier: a page box with no canvas in it.
//!
//! A strip under an adaptive policy mounts more pages than it rasterises — that
//! is the whole trade, and it only pays if the extra mounts are close to free.
//! This component is what "close to free" means: one `div` at the page's own
//! laid-out size, wearing the page's paper colour, its shadow and its texture,
//! with the document's shared miniature painted faintly into it. No canvas, so
//! no backing store, no 2d context and no GPU texture; no text layer, so no
//! spans; nothing registered with the engine, so no page state and no render
//! request. Eight of them mounted cost less than one full raster.
//!
//! The miniature is the engine's (see `public/engine/placeholder.ts`): one
//! representative page per document, rendered once and published as a CSS
//! custom property, so N placeholder boxes share one decoded image instead of
//! holding N copies of a page nobody is looking at. Until it lands — it is
//! built a beat after open, behind the reader's first page — the box is plain
//! paper, which is exactly what an unpainted page looked like before.
//!
//! What it is NOT is a lie about geometry. The main-axis extent is the
//! virtualizer's own number for the item, the same one the strip sums its
//! offsets from, and the cross axis follows the page's aspect ratio, so a
//! placeholder occupies precisely the box the real page will occupy when the
//! tier promotes it. Nothing shifts when a page arrives.

use leptos::prelude::*;
use reader_core::appearance::TextureMode;
use reader_core::view::Axis;

use pdf_core::pixel_grid::snap_px;

use crate::state::ReaderState;

#[component]
pub fn PdfPagePlaceholder(
    state: ReaderState,
    /// The strip's axis: it decides which of the two extents is the
    /// virtualizer's and which follows the page's proportions.
    axis: Axis,
    /// The item's index in the strip (0-based).
    index: usize,
    /// The main-axis extent the virtualizer lays this item out at — gap
    /// included on the vertical axis, the two page margins on the horizontal
    /// one, because both are part of the item's span in its size model.
    #[prop(into)]
    size: Signal<f64, LocalStorage>,
    /// The page texture mode, so a placeholder carries the same paper its
    /// promoted page will.
    #[prop(into)]
    texture: Signal<TextureMode>,
    /// Extra classes: the cross-axis `mx-auto` / `my-auto` that centres a page
    /// and degrades to start-alignment when it overflows.
    #[prop(default = String::new(), into)]
    class: String,
) -> impl IntoView {
    let texture = Memo::new(move |_| texture.get());
    let host_class = move || {
        let tex = texture.get().css_class();
        let mut classes = String::from("pdf-page pdf-page-placeholder");
        if let Some(tex) = tex {
            classes.push(' ');
            classes.push_str(tex);
        }
        if !class.is_empty() {
            classes.push(' ');
            classes.push_str(&class);
        }
        classes
    };

    // The box, in CSS px, snapped to the device-pixel grid for the same reason
    // the strip's offsets are: a placeholder that rounds differently from the
    // page it stands in for would show a hairline of backdrop at the joint the
    // moment it is promoted.
    let dims = Memo::new(move |_| {
        let extent = size.get().max(0.0);
        let intrinsic = state
            .document
            .content
            .metrics
            .intrinsic
            .with(|sizes| sizes.get(index).map(|size| (size.width, size.height)));
        // Page 1's size is the fallback for a page the document has not
        // reported yet — the same fallback the strips' size estimates use, so
        // the placeholder and the geometry model agree while nothing is known.
        let fallback = state
            .document
            .content
            .metrics
            .page1_size
            .get()
            .map(|size| (size.width, size.height));
        let (page_w, page_h) = match intrinsic.or(fallback) {
            Some((width, height)) if width > 0.0 && height > 0.0 => (width, height),
            _ => (0.0, 0.0),
        };
        match axis {
            Axis::Vertical => {
                let height = (extent - state.viewer.page_gap.get()).max(0.0);
                let width = if page_h > 0.0 {
                    page_w / page_h * height
                } else {
                    0.0
                };
                (snap_px(width), snap_px(height))
            }
            Axis::Horizontal => {
                let width = (extent - 2.0 * state.viewer.page_margin.get()).max(0.0);
                let height = if page_w > 0.0 {
                    page_h / page_w * width
                } else {
                    0.0
                };
                (snap_px(width), snap_px(height))
            }
        }
    });

    view! {
        // No id, no `data-reader-host`, no `data-host-page`: this box is not a
        // page host and must not answer to anything that looks for one. The
        // engine finds hosts by canvas id and the selection tracker by the host
        // attribute; a placeholder has neither, so a click on it lands on the
        // scroller behind, and a `closest` walk from inside it finds nothing to
        // mis-report a page number for.
        <div
            class=host_class
            aria-hidden="true"
            style=move || {
                let (width, height) = dims.get();
                format!("width:{width}px;height:{height}px")
            }
        ></div>
    }
}
