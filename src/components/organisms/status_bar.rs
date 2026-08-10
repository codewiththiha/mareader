//! Bottom status bar (page position only). OWNED BY branch B (viewer/chrome).
//! Phase 1 redesign: the filename and zoom readouts leave this bar (they move
//! to the toolbar and the zoom controls) — it now shows just the centered
//! `x / y` page counter.
//!
//! While `DocStatus::Ready`, the counter reads the live `viewer.page` /
//! `doc.num_pages`; any other status shows `– / –`. A mid-session open error
//! (toast system arrives in a later phase) is surfaced inline: when
//! `doc.error` is `Some`, the message replaces the counter, prefixed with a
//! warning glyph and truncating so it never breaks layout. It clears
//! automatically on the next open (which resets `doc.error`) — no timer.
//!
//! The error rides the footer's `mix-blend-difference` like the counter, so
//! its hue inverts against the page (red-400 reads red on dark pages, teal on
//! white ones); the glyph and semibold weight carry the warning signal even
//! when the hue inverts, and the blend keeps it legible over any color.
//!
//! Rendered as an overlay: the `<footer>` in ReaderView positions it
//! absolutely over the bottom of the viewer (no layout space of its own) and
//! makes it click-through. The bar has NO background — just the white text
//! floating over the page.
//!
//! `mix-blend-mode: difference` lives on the FOOTER (ReaderView), not here: a
//! blend only mixes with content inside its own stacking context, and the
//! footer (absolute + z-50) creates one — if the blend were on this div, it
//! would be isolated inside the footer and never reach the PDF pages. On the
//! footer, the white text inverts against whatever is beneath it, so it stays
//! readable (pops out) over any document color.

use leptos::prelude::*;

use crate::core::document::DocStatus;
use crate::core::state::AppState;

#[component]
pub fn StatusBar(state: AppState) -> impl IntoView {
    view! {
        <div class="flex h-8 items-center justify-center gap-3 px-3 text-xs text-white">
            {move || {
                if let Some(err) = state.doc.error.get() {
                    view! {
                        <span class="min-w-0 max-w-[60vw] truncate font-semibold text-red-400">
                            {format!("⚠ {err}")}
                        </span>
                    }
                    .into_any()
                } else {
                    view! {
                        <span class="whitespace-nowrap">
                            {match state.doc.status.get() {
                                DocStatus::Ready => format!(
                                    "{} / {}",
                                    state.viewer.page.get(),
                                    state.doc.num_pages.get()
                                ),
                                _ => "– / –".to_string(),
                            }}
                        </span>
                    }
                    .into_any()
                }
            }}
        </div>
    }
}
