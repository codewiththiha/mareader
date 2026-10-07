//! The library's one cover painter: cached art for an address, or the
//! surface's fallback.

use leptos::prelude::*;

#[component]
pub(crate) fn CoverThumb(
    state: crate::context::LibraryContext,
    /// Read per frame, so a relink moves the art key; empty paints
    /// the fallback.
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
                        // An image would hand the press to the engine's
                        // drag and swallow the release.
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
