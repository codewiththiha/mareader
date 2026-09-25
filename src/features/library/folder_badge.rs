//! The badge saying where a folder's books live, painted once for both shelf
//! surfaces.
//!
//! What the shelf HOLDS decides the badge — a folder whose books are stored
//! copies says so even when its seat still reads in place — and a folder with
//! no books of its own falls back to the governance mode, the promise its
//! import answered with. The fallback borrows the content side's sentences,
//! so a wording lives in `library_core::shelf::ContentKind` and nowhere else.

use leptos::prelude::*;

use library_core::shelf::ContentKind;

use crate::state::AppState;

/// `class` is the surface's own layout (`"folder-mode"` on the card,
/// `"folder-mode ml-2"` on the row); everything else about the badge is the
/// same on both, which is the point of there being one of them.
#[component]
pub(crate) fn FolderBadge(state: AppState, shelf_id: String, class: &'static str) -> impl IntoView {
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
