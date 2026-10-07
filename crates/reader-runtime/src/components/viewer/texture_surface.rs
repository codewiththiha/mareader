//! The reflowable reader's texture surface.

use leptos::prelude::*;

use crate::state::{ReaderState, TextureSignal};

/// The scroller's `texture-*` class, or `""` for a PDF.
pub fn texture_class(state: ReaderState) -> Memo<String> {
    let texture = use_context::<TextureSignal>()
        .expect("TextureSignal is provided by the pane realm, from its look");
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

/// The scroller's `--tx-zoom`, the live display scale.
pub fn zoom_style(state: ReaderState) -> Signal<String> {
    let display = state.viewer.zoom.display;
    Signal::derive(move || {
        let zoom = if state.reflowable() {
            display.get()
        } else {
            1.0
        };
        format!("--tx-zoom:{zoom};")
    })
}
