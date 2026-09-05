//! The viewer, organised by shape.
//!
//!   * [`Viewer`] — the single dispatch that turns the persisted `ViewMode` into
//!     a layout. It owns no scroll, zoom or format knowledge of its own; it picks
//!     one of four children and hands each the virtualizers it was given.
//!   * [`layouts`] — the four shapes themselves (single, spread, scroll
//!     horizontal, scroll vertical), thin on purpose: a layout says where its
//!     pages sit and nothing else.
//!   * [`shells`] — the parts that actually have to hold DOM: the scroll policy,
//!     the container binding, the overlay scrollbar, the progress strip. The
//!     layouts mount a shell rather than owning any of that.
//!   * [`controls`] — the reader's chrome: the bottom bar, the page indicator,
//!     the overlay scrollbar and the progress strip. The shells mount them; the
//!     reader page mounts the bar.
//!   * [`page_host`] — the one place a page's format is decided, so a layout can
//!     be shape without being conditionals.
//!   * [`texture_surface`] — the reflowable reader's texture carrier: the
//!     scroller class + zoom var (PDF pages carry their own, in `formats`).
//!   * [`refresh`] — the invalidation fingerprints: what makes a page's type, and
//!     so anything painted over it, move without the scroller moving.
//!
//! The reactive primitives these components read are NOT here: they live in
//! `src/state/reader/viewer.rs` (a plain signals bundle) and in `reader-core`'s
//! `view` module, and each component reads only the bundle it names in its props.
//!
//! [`layouts`]: layouts
//! [`shells`]: shells
//! [`controls`]: controls
//! [`page_host`]: page_host
//! [`refresh`]: refresh

pub mod controls;
pub mod layouts;
pub mod page_host;
pub mod refresh;
pub mod shells;
pub mod texture_surface;

pub use page_host::{PageSlot, UniversalPageHost, UniversalStreamHost, UniversalStripHost};

use leptos::prelude::*;
use reader_core::view::ViewMode;
use virtual_list_leptos::Virtualizer;

use crate::components::viewer::layouts::scroll_horizontal::ScrollHorizontalLayout;
use crate::components::viewer::layouts::scroll_vertical::ScrollVerticalLayout;
use crate::components::viewer::layouts::single::SingleLayout;
use crate::components::viewer::layouts::spread::SpreadLayout;
use crate::state::ReaderState;

#[component]
pub fn Viewer(
    state: ReaderState,
    /// The continuous (vertical) reader's virtualizer, shared with navigation
    /// sync and the zoom coordinator.
    virtualizer: Virtualizer,
    /// The horizontal strip's virtualizer (same role when mode is scroll-horizontal).
    h_virtualizer: Virtualizer,
    #[prop(into)]
    progress_visible: Signal<bool>,
) -> impl IntoView {
    // The virtualizers are parked in local (non-thread-safe) storage so the
    // reactive dispatch closure only has to capture Send-friendly handles,
    // resolving them lazily for the branch actually shown.
    let virtualizer_view = StoredValue::new_local(virtualizer);
    let h_virtualizer_view = StoredValue::new_local(h_virtualizer);
    let mode = state.viewer.mode;
    view! {
        {move || {
            match mode.get() {
                ViewMode::Single => view! {
                    <SingleLayout state=state progress_visible=progress_visible />
                }
                .into_any(),
                ViewMode::Spread => view! {
                    <SpreadLayout state=state progress_visible=progress_visible />
                }
                .into_any(),
                ViewMode::ScrollVertical => view! {
                    <ScrollVerticalLayout
                        state=state
                        virtualizer=virtualizer_view.get_value()
                        progress_visible=progress_visible
                    />
                }
                .into_any(),
                ViewMode::ScrollHorizontal => view! {
                    <ScrollHorizontalLayout
                        state=state
                        virtualizer=h_virtualizer_view.get_value()
                        progress_visible=progress_visible
                    />
                }
                .into_any(),
            }
        }}
    }
}
