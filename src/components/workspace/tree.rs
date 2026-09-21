//! The library tree: the shelves as the reader filed them, nested, with the
//! books each level holds — and the root level's unfiled books alongside the
//! root shelves. This is the hierarchy the library home already answers
//! (`children_of` / `members_of` / `find`), projected to a value-only shape
//! the sidebar renders and the host receives: no flat "All books" list, no
//! pseudo-shelf node, no book copied into every shelf it belongs to.

use std::collections::HashSet;

use serde::Serialize;

use library_core::book::{self, Book, Row};
use library_core::shelf::{self, Shelf, ALL_SHELF};
use library_core::sort::{self, SortKey};

use leptos::prelude::*;

use crate::state::AppState;

/// One node in the tree: a shelf with its own level below it, or a book leaf.
#[derive(Clone, Serialize, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TreeEntry {
    #[serde(rename = "folder")]
    Folder {
        id: String,
        title: String,
        children: Vec<TreeEntry>,
    },
    #[serde(rename = "book")]
    Book {
        id: String,
        title: String,
        format: String,
        missing: bool,
    },
}

impl TreeEntry {
    pub fn id(&self) -> &str {
        match self {
            TreeEntry::Folder { id, .. } => id,
            TreeEntry::Book { id, .. } => id,
        }
    }
    pub fn is_folder(&self) -> bool {
        matches!(self, TreeEntry::Folder { .. })
    }
    pub fn is_book(&self) -> bool {
        matches!(self, TreeEntry::Book { .. })
    }
}

fn format_name(format: reader_core::format::Format) -> String {
    match format {
        reader_core::format::Format::Pdf => "pdf",
        reader_core::format::Format::Text => "txt",
        reader_core::format::Format::Markdown => "md",
    }
    .to_string()
}

fn book_entry(book: &Book) -> TreeEntry {
    TreeEntry::Book {
        id: book.id.clone(),
        title: book.title(),
        format: format_name(book.format),
        missing: book.missing,
    }
}

fn folder_entry(
    books: &[Row],
    shelves: &[Shelf],
    shelf_id: &str,
    seen: &mut HashSet<String>,
) -> TreeEntry {
    let title = shelf::find(shelves, shelf_id)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| shelf_id.to_string());
    let mut children = Vec::new();
    for child in shelf::children_of(shelves, Some(shelf_id)) {
        if child.id == ALL_SHELF || !seen.insert(child.id.clone()) {
            continue;
        }
        children.push(folder_entry(books, shelves, &child.id, seen));
    }
    let members: Vec<String> = shelf::members_of(books, shelves, shelf_id)
        .into_iter()
        .map(str::to_string)
        .collect();
    let rows = sort::ordered(books, &members, SortKey::Manual, true);
    children.extend(book::book_rows(&rows).map(book_entry));
    TreeEntry::Folder {
        id: shelf_id.to_string(),
        title,
        children,
    }
}

pub fn library_roots(
    books: &[Row],
    shelves: &[Shelf],
    sort_key: SortKey,
    asc: bool,
) -> Vec<TreeEntry> {
    let mut seen = HashSet::new();
    let mut roots: Vec<TreeEntry> = Vec::new();
    for shelf in shelf::children_of(shelves, None) {
        if shelf.id == ALL_SHELF || !seen.insert(shelf.id.clone()) {
            continue;
        }
        roots.push(folder_entry(books, shelves, &shelf.id, &mut seen));
    }
    let unfiled: Vec<String> = shelf::members_of(books, shelves, ALL_SHELF)
        .into_iter()
        .map(str::to_string)
        .collect();
    let loose = sort::ordered(books, &unfiled, sort_key, asc);
    roots.extend(book::book_rows(&loose).map(book_entry));
    roots
}

/// Library region of the workspace sidebar — live off persisted library state.
/// Shows hierarchical shelves (no All Books) and books using same cover cache
/// and visual language as LibraryPage, compact for sidebar, draggable for split.
#[component]
pub fn WorkspaceLibraryTree(state: AppState) -> impl IntoView {
    let entries = Signal::derive(move || {
        let books = state.library.books.get();
        let shelves = state.library.shelves.get();
        let view = state.library.view.get();
        library_roots(&books, &shelves, view.sort, view.sort_asc)
    });

    // Split root into folders and loose books for better layout: folders as
    // full-width rows, loose books as 2-col grid of library-style cards.
    let folders = Signal::derive(move || {
        entries
            .get()
            .into_iter()
            .filter(|e| e.is_folder())
            .collect::<Vec<_>>()
    });
    let loose_books = Signal::derive(move || {
        entries
            .get()
            .into_iter()
            .filter(|e| e.is_book())
            .collect::<Vec<_>>()
    });

    view! {
        <section class="workspace-section">
            <h2 class="workspace-section-title">Library</h2>
            <Show when=move || !entries.get().is_empty() fallback=move || {
                view! { <p class="workspace-empty">Drop a document in to add it to your library.</p> }
            }>
                // Folders first, hierarchical
                <For
                    each=move || folders.get()
                    key=|entry: &TreeEntry| entry.id().to_string()
                    children=move |entry: TreeEntry| {
                        view! { <TreeNode state=state entry=entry /> }
                    }
                />
                // Loose books as library-style grid, no All Books pseudo-folder
                <Show when=move || !loose_books.get().is_empty()>
                    <div class="workspace-lib-grid">
                        <For
                            each=move || loose_books.get()
                            key=|entry: &TreeEntry| entry.id().to_string()
                            children=move |entry: TreeEntry| {
                                view! { <TreeNode state=state entry=entry /> }
                            }
                        />
                    </div>
                </Show>
            </Show>
        </section>
    }
}

#[component]
fn TreeNode(state: AppState, entry: TreeEntry) -> impl IntoView {
    match entry {
        TreeEntry::Folder { id: _, title, children } => {
            let open = RwSignal::new(true);
            let kids = Signal::derive(move || children.clone());
            let folder_kids = Signal::derive(move || {
                kids.get()
                    .into_iter()
                    .filter(|e| e.is_folder())
                    .collect::<Vec<_>>()
            });
            let book_kids = Signal::derive(move || {
                kids.get()
                    .into_iter()
                    .filter(|e| e.is_book())
                    .collect::<Vec<_>>()
            });
            view! {
                <div class="workspace-tree-node">
                    <button
                        class="workspace-shelf"
                        aria-expanded=move || open.get()
                        on:click=move |_| open.update(|o| *o = !*o)
                    >
                        <span class="workspace-shelf-chevron" aria-hidden="true">
                            {move || { if open.get() { "▾" } else { "▸" } }}
                        </span>
                        <span class="truncate">{title}</span>
                    </button>
                    <Show when=move || open.get()>
                        <div class="workspace-tree-kids">
                            <For
                                each=move || folder_kids.get()
                                key=|child: &TreeEntry| child.id().to_string()
                                children=move |child: TreeEntry| {
                                    view! { <TreeNode state=state entry=child /> }
                                }
                            />
                            <Show when=move || !book_kids.get().is_empty()>
                                <div class="workspace-lib-grid">
                                    <For
                                        each=move || book_kids.get()
                                        key=|child: &TreeEntry| child.id().to_string()
                                        children=move |child: TreeEntry| {
                                            view! { <TreeNode state=state entry=child /> }
                                        }
                                    />
                                </div>
                            </Show>
                        </div>
                    </Show>
                </div>
            }
            .into_any()
        }
        TreeEntry::Book { id, title, format, missing } => {
            view! {
                <WorkspaceLibraryBook state=state id=id title=title format=format missing=missing />
            }
            .into_any()
        }
    }
}

/// Compact library-style book for workspace filesystem.
/// - Same `state.library.covers` cache as LibraryPage
/// - Same title treatment, A4 cropped covers (not fixed squares)
/// - Compact row/card geometry, draggable for split via host MIME
#[component]
fn WorkspaceLibraryBook(
    state: AppState,
    id: String,
    title: String,
    format: String,
    missing: bool,
) -> impl IntoView {
    let book_id = id.clone();
    let open_id = id.clone();
    let cover_id = id.clone();
    let title_for_attr = title.clone();
    let format_for_fallback = format.clone();
    let format_for_badge = format.clone();
    let format_for_meta = format.clone();

    let book_row = Signal::derive(move || {
        let books = state.library.books.get();
        library_core::book::find_by_id(&books, &book_id).cloned()
    });

    let display_title = Signal::derive(move || {
        book_row
            .get()
            .map(|b| b.title())
            .unwrap_or_else(|| title.clone())
    });

    let author_or_format = Signal::derive(move || {
        book_row
            .get()
            .and_then(|b| b.author())
            .unwrap_or_else(|| format_for_meta.to_uppercase())
    });

    let cover_src = Signal::derive(move || {
        let books = state.library.books.get();
        let path = library_core::book::find_by_id(&books, &cover_id).map(|b| b.path().to_string());
        path.and_then(|p| state.library.covers.with(|c| c.get(&p).map(|cover| cover.data_url.clone())))
    });

    view! {
        <button
            class="workspace-book workspace-lib-book"
            data-book-id=id.clone()
            draggable=if missing { "false" } else { "true" }
            disabled=missing
            title=title_for_attr
            on:click=move |_| crate::services::document::open::open_book(state, open_id.clone())
        >
            <span class="workspace-lib-cover-wrap">
                <span class="workspace-lib-cover book-cover-crop" style="aspect-ratio: 210 / 297">
                    <Show
                        when=move || cover_src.get().is_some()
                        fallback=move || {
                            view! {
                                <span class="workspace-lib-cover-fallback">
                                    <span class="format-badge">{format_for_fallback.to_uppercase()}</span>
                                </span>
                            }
                        }
                    >
                        <img
                            class="workspace-lib-cover-img book-cover-img"
                            alt=""
                            src=move || cover_src.get().unwrap_or_default()
                            loading="lazy"
                            draggable="false"
                        />
                    </Show>
                </span>
            </span>
            <span class="workspace-lib-info">
                <span class="workspace-lib-title" title=move || display_title.get()>
                    {move || display_title.get()}
                </span>
                <span class="workspace-lib-meta truncate">
                    {move || author_or_format.get()}
                </span>
            </span>
            <span class="workspace-lib-format format-badge">{format_for_badge.to_uppercase()}</span>
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::sort::SortKey;
    use library_core::testkit::{markdown_row, row, shelf};

    #[test]
    fn the_root_level_holds_root_shelves_and_only_unfiled_books() {
        let books = vec![row("b1"), row("b2"), markdown_row("b3")];
        let shelves = vec![shelf("s1", "Programming", &["b1"], None), shelf("s2", "Rust", &[\"b2\"], Some(\"s1\"))];
        let roots = library_roots(&books, &shelves, SortKey::Manual, true);
        assert_eq!(roots.len(), 2);
        match &roots[0] {
            TreeEntry::Folder { id, title, children } => {
                assert_eq!(id, \"s1\");
                assert_eq!(title, \"Programming\");
                assert_eq!(children.len(), 2);
            }
            other => panic!(\"expected root shelf, got {other:?}\"),
        }
        match &roots[1] {
            TreeEntry::Book { id, .. } => assert_eq!(id, \"b3\"),
            other => panic!(\"expected unfiled book, got {other:?}\"),
        }
    }

    #[test]
    fn a_corrupt_blob_cannot_hang_the_tree() {
        let books = vec![row(\"b1\")];
        let mut shelves = [
            shelf(\"s1\", \"One\", &[], None),
            shelf(\"s2\", \"Two\", &[], None),
            shelf(\"s3\", \"Three\", &[], Some(\"s2\")),
        ];
        shelves[1].parent = Some(\"s3\".to_string());
        shelves[2].parent = Some(\"s2\".to_string());
        let roots = library_roots(&books, &shelves[..], SortKey::Manual, true);
        assert_eq!(roots.len(), 2);
    }
}
