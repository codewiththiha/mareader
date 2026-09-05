//! The DOM vocabulary the app shares with the imperative engine.
//!
//! The engine under `public/engine/` paints PDF pages into hosts the app
//! builds, and the two sides meet only in the DOM: attribute names, the values
//! those attributes carry, one class name, and the shape of element ids.
//! Neither compiler can see the other, so a rename here is not an error there —
//! it is a query that quietly returns nothing. A host that stops advertising
//! itself is a selection that never becomes an "Explain" pill; an id that changes
//! shape is a canvas that never finds the page it belongs to.
//!
//! What lives here is the half Rust uses as a *value*: in a `closest` selector,
//! in a `get_attribute`, in a comparison. Two more names cross the boundary and
//! cannot live here, because Leptos takes an attribute's NAME from the markup
//! and only its value from an expression:
//!
//! * `data-host-page` — the 1-based page a host paints, written by
//!   `crate::components::formats::pdf::canvas`,
//!   `crate::components::formats::reflow::page` and
//!   `crate::components::formats::reflow::stream`. It is the first thing the
//!   engine's selection tracker asks a host for; the id shape is only the
//!   fallback.
//! * `data-ai-popover` — marks the AI pill's root so that a press inside it is
//!   not read as a click that clears the selection
//!   (`crate::components::ai::selection_pill`).
//!
//! Element ids are built, not spelled: `crate::components::viewer::page_host`
//! owns [`crate::components::viewer::page_host::host_id_for_mode`] (the
//! `sp-`/`dp-`/`hp-`/`cont-` hosts and their `-cv` canvases) and
//! [`crate::components::viewer::page_host::block_row_id`], and
//! `crate::components::formats::pdf::strip` owns the `-wrap` rows around a
//! vertical strip's pages. Those shapes are part of this contract even though
//! the strings are not here — putting them here would mean writing them twice
//! in Rust as well.
//!
//! The engine's half of the table is `public/engine/dom-contract.ts`, and
//! `tools/check-dom-contract.ts` fails CI when the two halves disagree. It
//! reads the id shapes out of the builders' `format!` strings rather than
//! trusting a second copy of them, so a rename in either language is caught by
//! the other.

/// The attribute every reader page host carries, naming the format family that
/// painted it. The engine's selection tracker and the app's own captures both
/// find their host through it, so a new format adds one attribute and joins.
pub const HOST_ATTR: &str = "data-reader-host";

/// The [`HOST_ATTR`] value a reflowable page or stream block carries.
pub const HOST_REFLOW: &str = "reflow";

/// The [`HOST_ATTR`] value a PDF page carries.
pub const HOST_PDF: &str = "pdf";

/// On a rendered block: which block of the document it is, in document order.
///
/// The identity half of the two handles a reflowable mark has on the DOM — the
/// engine's selection tracker walks up to it with `closest`, and a capture reads
/// the block number off it — and it is what makes the paginated modes and the
/// continuous stream resolve identically. The lookup half is the element id,
/// [`crate::components::viewer::page_host::block_row_id`], which the projection
/// resolves per mark per refresh.
pub const BLOCK_INDEX_ATTR: &str = "data-block-index";

/// The class of the still-bitmap overlay a zoom stretches while a re-render is
/// on its way.
///
/// `crate::components::formats::pdf::canvas_host` creates and removes it; the
/// engine's teardown clears any that outlived a document, because a snapshot
/// left in a recycled host would show the previous page's pixels.
pub const PAGE_SNAPSHOT_CLASS: &str = "page-snapshot";

/// The class of the text layer inside a PDF host.
///
/// The app builds the empty div (`crate::components::formats::pdf::canvas`); the
/// engine fills it with positioned spans, replaces it on a zoom, and queries it
/// when a selection has to know which layer of the document it is in. Both sides
/// look it up by this name.
pub const TEXT_LAYER_CLASS: &str = "textLayer";
