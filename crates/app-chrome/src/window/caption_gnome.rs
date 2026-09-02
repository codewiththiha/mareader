//! The circular GNOME-style caption cluster (Linux).
//!
//! Same three commands, same `maximized` glyph swap as the Windows cluster —
//! only the shape differs: 24px circles with a translucent fill that tracks
//! the theme ink, the shape a GNOME header bar draws. The style lives in
//! `styles/components/title_bar.css` under `.gnome-btn`.

use leptos::prelude::*;

use crate::icon::{Icon, IconName};
use crate::window::caption::{close, minimize, toggle_maximize};

#[component]
pub fn GnomeControls(maximized: RwSignal<bool>) -> impl IntoView {
    view! {
        // NO data-tauri-drag-region in here — these must stay clickable.
        <div class="window-controls gnome">
            <button type="button" class="gnome-btn" title="Minimize" aria-label="Minimize" on:click=move |_| minimize()>
                <Icon name=IconName::WindowMinimize size=12 />
            </button>

            <button
                type="button"
                class="gnome-btn"
                title=move || if maximized.get() { "Restore" } else { "Maximize" }
                aria-label=move || if maximized.get() { "Restore" } else { "Maximize" }
                on:click=move |_| toggle_maximize()
            >
                {move || {
                    if maximized.get() {
                        view! { <Icon name=IconName::WindowRestore size=12 /> }.into_any()
                    } else {
                        view! { <Icon name=IconName::WindowMaximize size=12 /> }.into_any()
                    }
                }}
            </button>

            <button type="button" class="gnome-btn gnome-btn-close" title="Close" aria-label="Close" on:click=move |_| close()>
                <Icon name=IconName::Close size=12 />
            </button>
        </div>
    }
}
