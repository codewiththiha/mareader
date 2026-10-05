//! The library's one cover painter: the cached art for an address, or the
//! fallback the calling surface names. The grid's card, the list's row and the
//! search bar's thumb each hand-rolled this subscription before.

use leptos::prelude::*;

#[component]
pub(crate) fn CoverThumb(
    state: crate::context::LibraryContext,
    /// Read per frame, so a relink moves the art key and the surface follows.
    /// Empty (the beat between a removal and the list catching up) paints the
    /// fallback.
    path: Signal<String>,
    alt: Signal<String>,
    img_class: &'static str,
    /// A `Callback` rather than children: it is the prop's second closure.
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
                        // An image is natively draggable: a press on the
                        // cover would hand the pointer to the engine's own
                        // drag, which used to swallow the release.
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
