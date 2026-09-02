//! Row 2 of the sidebar: book identity (cover + title + author + info).
//! Always visible while the sidebar is open and a document is Ready.

use leptos::prelude::*;

use pdf_engine::types::DocStatus;
use app_chrome::icon::{Icon, IconName};
use crate::state::library::CoverMap;
use crate::state::{NO_DOCUMENT, ReaderState};


#[component]
pub(crate) fn BookInfo(
    reader: ReaderState,
    covers: RwSignal<CoverMap>,
) -> impl IntoView {
    view! {
        <Show when=move || reader.document.status.get() == DocStatus::Ready>
            <div
                class="flex items-center gap-3 border-b border-line px-3 pb-3"
                data-tauri-drag-region="true"
            >
                {move || {
                    let path = reader.document.path.get().as_deref().unwrap_or(NO_DOCUMENT).to_string();
                    let cover = covers.with(|covers| covers.get(&path).cloned());
                    match cover {
                        Some(c) => view! {
                            <img
                                class="h-12 w-10 rounded-sm border border-line/60 object-cover"
                                src=c.data_url.clone()
                                alt="Cover"
                                loading="lazy"
                            />
                        }
                        .into_any(),
                        None => view! {
                            <div class="flex h-12 w-10 items-center justify-center rounded-sm border border-line bg-surface">
                                <Icon name=IconName::Open size=14 />
                            </div>
                        }
                        .into_any(),
                    }
                }}
                <div class="min-w-0 flex-1" data-tauri-drag-region="true">
                    <p
                        class="truncate text-sm font-semibold text-ink"
                        data-tauri-drag-region="true"
                    >
                        {move || reader.document.display_name()}
                    </p>
                    <p class="truncate text-xs text-muted">
                        {move || reader.document.author.get().unwrap_or_default()}
                    </p>
                </div>
                <span
                    title=move || reader.document.path.get().as_deref().unwrap_or(NO_DOCUMENT).to_string()
                    class="text-muted"
                >
                    <Icon name=IconName::More size=14 />
                </span>
            </div>
        </Show>
    }
}
