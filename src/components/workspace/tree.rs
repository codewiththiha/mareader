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
    /// The stable identity a keyed list re-renders against.
    pub fn id(&self) -> &str {
        match self {
            TreeEntry::Folder { id, .. } => id,
            TreeEntry::Book { id, .. } => id,
        }
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

/// One shelf's level: its sub-shelves first, then its own books in the
/// reader's manual order — the same answer the library's shelf view gives.
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
        // A blob that already carries a cycle would otherwise spin here
        // forever; a level the reader cannot have opened stays out.
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

/// The root level: the reader's root shelves, then the books no shelf holds,
/// sorted the way the library home sorts its root — one answer with the full
/// library page.
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

/// The Library region of the workspace sidebar: the tree above, live off the
/// persisted library state — a book the readers just opened or filed
/// appears here without a refresh, because the state it renders IS the
/// library's state.
#[component]
pub fn WorkspaceLibraryTree(state: AppState) -> impl IntoView {
    let entries = Signal::derive(move || {
        let books = state.library.books.get();
        let shelves = state.library.shelves.get();
        let view = state.library.view.get();
        library_roots(&books, &shelves, view.sort, view.sort_asc)
    });
    view! {
        <section class="workspace-section">
            <h2 class="workspace-section-title">Library</h2>
            <Show when=move || !entries.get().is_empty() fallback=move || {
                view! { <p class="workspace-empty">Drop a document in to add it to your library.</p> }
            }>
                <For
                    each=move || entries.get()
                    key=|entry: &TreeEntry| entry.id().to_string()
                    children=move |entry: TreeEntry| {
                        view! { <TreeNode state=state entry=entry /> }
                    }
                />
            </Show>
        </section>
    }
}

/// One node: a shelf is a toggle plus its level below; a book is the row the
/// host's drag session binds to (`data-book-id`) and that opens straight
/// through the library's own open flow.
#[component]
fn TreeNode(state: AppState, entry: TreeEntry) -> impl IntoView {
    match entry {
        TreeEntry::Folder { id: _, title, children } => {
            let open = RwSignal::new(true);
            let kids = Signal::derive(move || children.clone());
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
                                each=move || kids.get()
                                key=|child: &TreeEntry| child.id().to_string()
                                children=move |child: TreeEntry| {
                                    view! { <TreeNode state=state entry=child /> }
                                }
                            />
                        </div>
                    </Show>
                </div>
            }
            .into_any()
        }
        TreeEntry::Book { id, title, format, missing } => {
            let open_id = id.clone();
            let cover_id = id.clone();
            // Same cover cache as the full LibraryPage — compact presentation but identical source.
            let cover_src = Signal::derive(move || {
                let books = state.library.books.get();
                let path = library_core::book::find_by_id(&books, &cover_id).map(|b| b.path().to_string());
                path.and_then(|p| state.library.covers.with(|c| c.get(&p).map(|cover| cover.data_url.clone())))
            });
            view! {
                <button
                    class="workspace-book"
                    data-book-id=id.clone()
                    draggable=if missing { "false" } else { "true" }
                    disabled=missing
                    title=title.clone()
                    on:click=move |_| crate::services::document::open::open_book(state, open_id.clone())
                >
                    <span class="workspace-book-cover">
                        <Show when=move || cover_src.get().is_some() fallback=move || view! { <span class="format-badge">{format.to_uppercase()}</span> }>
                            <img class="workspace-book-cover-img" alt="" src=move || cover_src.get().unwrap_or_default() loading="lazy" />
                        </Show>
                    </span>
                    <span class="truncate flex-1 text-left">{title}</span>
                    <span class="format-badge">{format.to_uppercase()}</span>
                </button>
            }
            .into_any()
        }
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
        let shelves = vec![shelf("s1", "Programming", &["b1"], None), shelf("s2", "Rust", &["b2"], Some("s1"))];
        let roots = library_roots(&books, &shelves, SortKey::Manual, true);

        assert_eq!(roots.len(), 2);
        match &roots[0] {
            TreeEntry::Folder { id, title, children } => {
                assert_eq!(id, "s1");
                assert_eq!(title, "Programming");
                assert_eq!(children.len(), 2);
                match &children[0] {
                    TreeEntry::Folder { id, .. } => assert_eq!(id, "s2"),
                    other => panic!("expected the nested shelf first, got {other:?}"),
                }
                match &children[1] {
                    TreeEntry::Book { id, .. } => assert_eq!(id, "b1"),
                    other => panic!("expected the shelf's own book, got {other:?}"),
                }
            }
            other => panic!("expected the root shelf, got {other:?}"),
        }
        // b1 and b2 are filed, so only b3 is loose — no book appears twice.
        match &roots[1] {
            TreeEntry::Book { id, .. } => assert_eq!(id, "b3"),
            other => panic!("expected the unfiled book, got {other:?}"),
        }
    }

    #[test]
    fn a_corrupt_blob_cannot_hang_the_tree() {
        let books = vec![row("b1")];
        let mut shelves = [
            shelf("s1", "One", &[], None),
            shelf("s2", "Two", &[], None),
            shelf("s3", "Three", &[], Some("s2")),
        ];
        // Corrupt the blob into a cycle: s2 hangs under s3 and s3 under s2.
        shelves[1].parent = Some("s3".to_string());
        shelves[2].parent = Some("s2".to_string());
        let roots = library_roots(&books, &shelves[..], SortKey::Manual, true);
        // The cycle gives its shelves no root to hang from, so the walk
        // terminates at s1 — empty, and not spinning — with b1 the only
        // loose book.
        assert_eq!(roots.len(), 2);
        match &roots[0] {
            TreeEntry::Folder { id, children, .. } => {
                assert_eq!(id, "s1");
                assert!(children.is_empty());
            }
            other => panic!("expected the reachable root shelf, got {other:?}"),
        }
        match &roots[1] {
            TreeEntry::Book { id, .. } => assert_eq!(id, "b1"),
            other => panic!("expected the unfiled book, got {other:?}"),
        }
    }
}
