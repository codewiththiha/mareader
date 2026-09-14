//! The library's one cover painter: the cached art for an address, or the fallback the calling
//! surface names. The grid's card, the list's row and the search bar's thumb each hand-rolled
//! the same subscription before this.

use leptos::prelude::*;

use crate::state::AppState;

#[component]
pub(crate) fn CoverThumb(
    state: AppState,
    /// Read on the frame it is asked for, so a relink moves a book's art key and the surface follows. Empty (the beat between a removal and the list catching up) paints the fallback.
    path: Signal<String>,
    alt: Signal<String>,
    img_class: &'static str,
    /// A `Callback` rather than children because it is the prop's SECOND closure.
    #[prop(optional)]
    fallback: Option<Callback<(), AnyView>>,
) -> impl IntoView {
    view! {
        {move || {
            match state
                .library
                .covers
                .with(|covers| covers.get(&path.get()).cloned())
            {
                Some(cover) => {
                    view! {
                        // An image is natively draggable, so a press on the cover would hand the pointer to the engine's own drag — the one that used to swallow the release.
                        <img
                            class=img_class
                            src=cover.data_url.clone()
                            alt=alt.get()
                            loading="lazy"
                            draggable="false"
                        />
                    }
                        .into_any()
                }
                None => fallback
                    .map(|f| f.run(()))
                    .unwrap_or_else(|| ().into_any()),
            }
        }}
    }
}
