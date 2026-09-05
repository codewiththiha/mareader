//! The reflowable reader's texture surface.
//!
//! There are two texture carriers, one per format family:
//!   * PDF — each page carries its own pattern composited over its raster
//!     (see `formats::pdf::canvas` and the PER-PAGE section of
//!     `styles/textures.css`);
//!   * reflowable (text/Markdown) — a page is a transparent frame, so a
//!     per-page rectangle would stamp a page-shaped seam onto the one-piece
//!     paper. The texture rides the scroller that IS the surface — the
//!     paginated shell, the continuous stream, the reflowable strip — as a
//!     background-image with `background-attachment: local`: one repeating
//!     pattern behind all type, scrolling with it, no blend or z-index
//!     machinery (see the SCROLLER TEXTURE section of `styles/textures.css`).
//!
//! This module is the reflowable half's only Rust: it hands each scroller
//! the `texture-*` class it needs and the `--tx-zoom` the stylesheet scales
//! the pitch with. For a PDF document the class is empty — the PDF scroller
//! paints the chrome surface and must never carry a pattern.

use leptos::prelude::*;

use crate::state::{ReaderState, TextureSignal};

/// The `texture-*` class for a reflowable document's scroller, or `""` for a
/// PDF document (whose pages own their texture; the class must not land on
/// the PDF scroller, which paints the chrome surface).
pub fn texture_class(state: ReaderState) -> Memo<String> {
    let texture =
        use_context::<TextureSignal>().expect("TextureSignal must be provided by app bootstrap");
    Memo::new(move |_| {
        if !state.reflowable() {
            return String::new();
        }
        match texture.get().css_class() {
            Some(class) => class.to_string(),
            None => String::new(),
        }
    })
}

/// The scroller's inline `--tx-zoom`: the same live display scale the type
/// resolves through (`--ts` on the hosts), so the texture pitch zooms in
/// lockstep with the text. `1` for PDF, whose pages carry their own scale
/// factor.
pub fn zoom_style(state: ReaderState) -> Signal<String> {
    let display = state.viewer.zoom.display;
    Signal::derive(move || {
        let zoom = if state.reflowable() { display.get() } else { 1.0 };
        format!("--tx-zoom:{zoom};")
    })
}
