//! The square, full-height Windows caption cluster.
//!
//! Sizing follows the platform convention rather than the app's icon
//! buttons: 46px-wide full-height hit targets flush against the window
//! edge, no rounding, no borders, hover-only backgrounds. The style lives
//! in `styles/components/title_bar.css` under `.win-btn`.

use leptos::prelude::*;

use crate::icon::{Icon, IconName};
use crate::window::caption::{close, minimize, toggle_maximize};

#[component]
pub fn WindowsControls(maximized: RwSignal<bool>) -> impl IntoView {
    view! {
        // NO data-tauri-drag-region in here — these must stay clickable.
        <div class="window-controls">
            <button type="button" class="win-btn" title="Minimize" aria-label="Minimize" on:click=move |_| minimize()>
                <Icon name=IconName::WindowMinimize size=14 />
            </button>

            <button
                type="button"
                class="win-btn"
                title=move || if maximized.get() { "Restore" } else { "Maximize" }
                aria-label=move || if maximized.get() { "Restore" } else { "Maximize" }
                on:click=move |_| toggle_maximize()
            >
                {move || {
                    if maximized.get() {
                        view! { <Icon name=IconName::WindowRestore size=14 /> }.into_any()
                    } else {
                        view! { <Icon name=IconName::WindowMaximize size=14 /> }.into_any()
                    }
                }}
            </button>

            <button type="button" class="win-btn win-btn-close" title="Close" aria-label="Close" on:click=move |_| close()>
                <Icon name=IconName::Close size=14 />
            </button>
        </div>
    }
}
