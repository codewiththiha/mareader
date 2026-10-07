//! The badge saying where a folder's books live.

use leptos::prelude::*;

use library_core::shelf::ContentKind;

/// `class` is the surface's own layout; the rest is shared.
#[component]
pub(crate) fn FolderBadge(
    state: crate::context::LibraryContext,
    shelf_id: String,
    class: &'static str,
) -> impl IntoView {
    let content_id = shelf_id.clone();
    let content = Signal::derive(move || state.library.shelf_content_kind(&content_id));
    let mode = Signal::derive(move || state.library.shelf_mode(&shelf_id));
    move || {
        let kind = content.get();
        let words = if kind != ContentKind::Empty {
            Some((
                kind.badge().unwrap_or_default(),
                kind.tooltip().unwrap_or_default(),
                kind.is_mixed(),
            ))
        } else {
            mode.get().map(|mode| {
                let sentence = if mode.copies_files() {
                    ContentKind::Stored
                } else {
                    ContentKind::OnDisk
                };
                (mode.badge(), sentence.tooltip().unwrap_or_default(), false)
            })
        };
        words.map(|(badge, title, mixed)| {
            view! {
                <span class=class class=("folder-mode-mixed", mixed) title=title>
                    {badge}
                </span>
            }
        })
    }
}
