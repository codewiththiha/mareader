//! The Library panel's view: the open-tabs strip (only while the workspace
//! is split) over the library tree. Rows are one line each — a glyph, the
//! name truncated to the rail, a format badge — at a fixed 28px so a long
//! library stays a compact list.
//!
//! A file row is the workspace's split-drag source. A mouse or pen press
//! ARMS the host's drag session (nothing is measured, nothing moves); past
//! the shared threshold the session is live and the host's window listeners
//! follow the pointer onto the workspace, where the drop preview shows the
//! split it will make. No pointer capture and no HTML5 drag: the pointer has
//! to leave the rail, and an OS drag is the library route's import, never
//! this. Touch never drags. A press that stays a click is the row's click,
//! which does what the Workspace setting says.

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use app_chrome::icon::{Icon, IconName};
use reader_core::settings::LibraryClick;

use super::super::ReaderHost;
use super::super::model::DocumentId;
use super::{LibraryFile, Line, OpenHow, OpenTab, badge_of_format};

/// A row's left padding: the outline panel's rule — a 12px step with a cap
/// that always leaves the name room in the 288px rail.
fn indent_px(depth: usize) -> usize {
    const BASE: usize = 8;
    const STEP: usize = 12;
    const INDENT_MAX: usize = 96;
    BASE + (depth * STEP).min(INDENT_MAX)
}

/// The chevron's width plus its gap: a file lines its glyph up under its
/// folder's name.
const CHEVRON_PX: usize = 18;

pub(super) fn library_panel(host: ReaderHost, shown: Signal<bool>) -> AnyView {
    let state = host.library();
    // The store is read when the panel becomes the rail's visible one (and
    // not before: a rail that never shows the Library reads nothing).
    Effect::new(move |_| {
        if shown.get() {
            untrack(|| state.refresh());
        }
    });
    let split = Signal::derive(move || host.pane_count() > 1);

    // The documents the panes show, and the active pane's: a row names the
    // file a pane is reading. Tracked on each pane's status, because a
    // pane's document address is read untracked.
    let open_docs = Memo::new(move |_| {
        host.manager
            .placed()
            .into_iter()
            .filter_map(|id| {
                let pane = host.manager.pane(id)?;
                let _ = pane.surface().status.try_get();
                pane.document()
            })
            .collect::<Vec<_>>()
    });
    let active_doc = Memo::new(move |_| {
        host.manager.active_pane().and_then(|pane| {
            let _ = pane.surface().status.try_get();
            pane.document()
        })
    });

    let scroller: NodeRef<html::Div> = NodeRef::new();
    // Back where it was: the rail remounts on every active-pane change.
    Effect::new(move |_| {
        if let Some(el) = scroller.get() {
            el.set_scroll_top(untrack(|| state.scroll()));
        }
    });

    view! {
        <div class="flex min-h-0 flex-1 flex-col" data-library-panel="">
            <Show when=move || split.get()>{move || open_tabs_view(host)}</Show>
            <div class="flex shrink-0 items-center px-3 pb-1 pt-2.5 text-[11px] font-semibold uppercase tracking-wide text-muted">
                "Library"
            </div>
            <div
                node_ref=scroller
                role="tree"
                aria-label="Library"
                class="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-1.5 pb-2"
                on:scroll=move |_| {
                    if let Some(el) = scroller.get_untracked() {
                        state.remember_scroll(el.scroll_top());
                    }
                }
                on:keydown=move |ev: web_sys::KeyboardEvent| step_focus(&ev, scroller)
            >
                <Show when=move || !state.is_empty() fallback=empty_view>
                    <For
                        each=move || state.lines()
                        key=Line::key
                        children=move |line| line_view(host, line, open_docs, active_doc)
                    />
                </Show>
            </div>
        </div>
    }
    .into_any()
}

/// ArrowUp / ArrowDown walk the rows, like an explorer.
fn step_focus(ev: &web_sys::KeyboardEvent, scroller: NodeRef<html::Div>) {
    let step: i32 = match ev.key().as_str() {
        "ArrowDown" => 1,
        "ArrowUp" => -1,
        _ => return,
    };
    let Some(root) = scroller.get_untracked() else {
        return;
    };
    let Ok(rows) = root.query_selector_all("[data-lib-row]") else {
        return;
    };
    let focused = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element());
    let count = rows.length() as i32;
    let at = (0..count).find(|i| {
        rows.item(*i as u32)
            .zip(focused.as_ref())
            .is_some_and(|(row, el)| row.is_same_node(Some(&**el)))
    });
    let next = match at {
        Some(i) => (i + step).clamp(0, count - 1),
        None if step > 0 => 0,
        None => count - 1,
    };
    if let Some(row) = rows
        .item(next.max(0) as u32)
        .and_then(|n| n.dyn_into::<web_sys::HtmlElement>().ok())
    {
        ev.prevent_default();
        let _ = row.focus();
    }
}

fn empty_view() -> impl IntoView {
    view! {
        <div class="flex flex-col items-center gap-1.5 px-6 py-10 text-center">
            <Icon name=IconName::Library size=20 class="text-muted" />
            <p class="text-sm text-ink">"Your library is empty"</p>
            <p class="text-xs text-muted">"Files you add on the Library page appear here."</p>
        </div>
    }
}

fn line_view(
    host: ReaderHost,
    line: Line,
    open_docs: Memo<Vec<DocumentId>>,
    active_doc: Memo<Option<DocumentId>>,
) -> AnyView {
    match line {
        Line::Folder {
            id,
            name,
            depth,
            count,
            ..
        } => folder_view(host, id, name, depth, count).into_any(),
        Line::File { file, depth } => {
            file_view(host, file, depth, open_docs, active_doc).into_any()
        }
    }
}

fn folder_view(
    host: ReaderHost,
    id: String,
    name: String,
    depth: usize,
    count: usize,
) -> impl IntoView {
    let state = host.library();
    let key = StoredValue::new(id.clone());
    let open = Signal::derive(move || key.with_value(|id| state.is_open(id)));
    let title = name.clone();
    view! {
        <button
            type="button"
            role="treeitem"
            aria-expanded=move || open.get().to_string()
            data-lib-row=""
            title=title
            class="flex h-7 w-full items-center gap-1.5 rounded-md pr-2 text-left text-[13px] text-ink \
                   hover:bg-line/60 focus:outline-none focus-visible:ring-1 focus-visible:ring-inset \
                   focus-visible:ring-accent"
            style:padding-left=format!("{}px", indent_px(depth))
            on:click=move |_| key.with_value(|id| state.toggle(id))
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                let want = match ev.key().as_str() {
                    "ArrowRight" => true,
                    "ArrowLeft" => false,
                    _ => return,
                };
                ev.prevent_default();
                key.with_value(|id| state.set_open(id, want));
            }
        >
            <span
                class="flex w-3 shrink-0 justify-center text-muted transition-transform duration-150 motion-reduce:transition-none"
                class=("-rotate-90", move || !open.get())
            >
                <Icon name=IconName::ChevronDown size=12 />
            </span>
            <Icon name=IconName::Folder size=14 class="shrink-0 text-muted" />
            <span class="min-w-0 flex-1 truncate">{name}</span>
            <span class="shrink-0 text-[11px] tabular-nums text-muted">{count}</span>
        </button>
    }
}

fn file_view(
    host: ReaderHost,
    file: LibraryFile,
    depth: usize,
    open_docs: Memo<Vec<DocumentId>>,
    active_doc: Memo<Option<DocumentId>>,
) -> impl IntoView {
    let state = host.library();
    let document = DocumentId::from_launch(Some(&file.book_id), &file.path);
    let doc_open = document.clone();
    let is_open = Signal::derive(move || {
        doc_open
            .as_ref()
            .is_some_and(|doc| open_docs.with(|docs| docs.contains(doc)))
    });
    let is_active = Signal::derive(move || document.is_some() && active_doc.get() == document);
    let dragged_path = file.path.clone();
    let dragged = Signal::derive(move || host.dragged_path().as_deref() == Some(&dragged_path));
    let missing = file.missing;
    let title = if missing {
        format!("{} — missing; relink it on the Library page", file.name)
    } else {
        format!("{}\n{}", file.name, file.path)
    };
    let (name, badge, path) = (file.name.clone(), file.badge.clone(), file.path.clone());
    let row = StoredValue::new(file);
    view! {
        <button
            type="button"
            role="treeitem"
            aria-current=move || is_active.get().then_some("true")
            aria-disabled=missing.then_some("true")
            data-lib-row=""
            data-lib-file=path
            title=title
            class="flex h-7 w-full touch-none select-none items-center gap-1.5 rounded-md pr-2 \
                   text-left text-[13px] transition-opacity duration-100 focus:outline-none \
                   focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent"
            class=("bg-line", move || is_active.get())
            class=("font-medium", move || is_active.get())
            class=("text-ink", move || is_active.get() || is_open.get())
            class=("text-muted", move || !is_active.get() && !is_open.get())
            class=("hover:bg-line/60", move || !is_active.get())
            class=("hover:text-ink", move || !is_active.get())
            class=("opacity-40", move || missing || dragged.get())
            style:padding-left=format!("{}px", indent_px(depth) + CHEVRON_PX)
            on:pointerdown=move |ev: web_sys::PointerEvent| {
                if missing || ev.button() != 0 || ev.pointer_type() == "touch" {
                    return;
                }
                state.clear_swallowed();
                // No text selection, no native drag: the host's session is
                // the only drag there is.
                ev.prevent_default();
                let at = (f64::from(ev.client_x()), f64::from(ev.client_y()));
                row.with_value(|file| host.library_press(file, at));
            }
            on:click=move |ev: web_sys::MouseEvent| {
                // A keyboard activation has no pointer detail and no drag
                // before it.
                let keyboard = ev.detail() == 0;
                if (!keyboard && state.take_swallowed()) || missing {
                    return;
                }
                let click = host
                    .session
                    .settings
                    .try_with_untracked(|s| s.workspace.library_click)
                    .unwrap_or_default();
                let how = match click {
                    LibraryClick::Replace => Some(OpenHow::Here),
                    LibraryClick::Split => Some(OpenHow::Beside),
                    LibraryClick::DragOnly => keyboard.then_some(OpenHow::Beside),
                };
                if let Some(how) = how {
                    row.with_value(|file| host.library_open(file, how));
                }
            }
        >
            <Icon name=IconName::File size=13 class="shrink-0 opacity-70" />
            <span class="min-w-0 flex-1 truncate">{name}</span>
            <Show when=move || is_open.get() && !is_active.get()>
                <span aria-hidden="true" class="h-1.5 w-1.5 shrink-0 rounded-full bg-accent/70" />
            </Show>
            <span class="shrink-0 rounded px-1 text-[10px] font-semibold leading-4 tracking-wide text-muted ring-1 ring-inset ring-line">
                {badge}
            </span>
        </button>
    }
}

/// The open panes, as tabs: a click focuses one, its × closes it. They
/// never drag — the tree is the only split source.
fn open_tabs_view(host: ReaderHost) -> impl IntoView {
    view! {
        <div class="shrink-0 border-b border-line pb-2">
            <div class="flex items-center justify-between px-3 pb-1 pt-2.5 text-[11px] font-semibold uppercase tracking-wide text-muted">
                <span>"Open"</span>
                <span class="font-normal tabular-nums">{move || host.pane_count()}</span>
            </div>
            <div
                role="tablist"
                aria-label="Open panes"
                aria-orientation="vertical"
                class="flex flex-col gap-0.5 px-1.5"
            >
                <For
                    each=move || host.open_tabs()
                    key=|tab| tab.id
                    children=move |tab| tab_view(host, tab)
                />
            </div>
        </div>
    }
}

fn tab_view(host: ReaderHost, tab: OpenTab) -> impl IntoView {
    let OpenTab { id, name } = tab;
    let active = Signal::derive(move || host.manager.active() == Some(id));
    let label = Signal::derive(move || name.try_get().unwrap_or_default());
    // The format follows the document: a pane that replaced its document
    // renames itself, and the badge is re-read with the name.
    let badge = move || {
        label.track();
        host.manager
            .pane(id)
            .map_or("", |pane| badge_of_format(pane.format()))
    };
    view! {
        <div
            data-open-tab=id.get()
            class="group relative flex h-8 items-center rounded-md transition-colors"
            class=("bg-line", move || active.get())
            class=("hover:bg-line/60", move || !active.get())
        >
            <Show when=move || active.get()>
                <span aria-hidden="true" class="absolute bottom-1.5 left-0 top-1.5 w-0.5 rounded-full bg-accent" />
            </Show>
            <button
                type="button"
                role="tab"
                aria-selected=move || active.get().to_string()
                title=move || label.get()
                class="flex h-full min-w-0 flex-1 items-center gap-2 rounded-md pl-2.5 pr-1 text-left \
                       text-[13px] focus:outline-none focus-visible:ring-1 focus-visible:ring-inset \
                       focus-visible:ring-accent"
                class=("text-ink", move || active.get())
                class=("font-medium", move || active.get())
                class=("text-muted", move || !active.get())
                on:click=move |_| {
                    let _ = host.set_active(id);
                }
            >
                <span class="w-8 shrink-0 text-[10px] font-semibold tracking-wide text-muted">
                    {badge}
                </span>
                <span class="min-w-0 flex-1 truncate">{move || label.get()}</span>
            </button>
            <button
                type="button"
                data-open-tab-close=id.get()
                aria-label=move || format!("Close {}", label.get())
                title="Close pane"
                class="mr-1 flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted \
                       opacity-0 transition-opacity hover:bg-line hover:text-ink focus:outline-none \
                       focus-visible:opacity-100 focus-visible:ring-1 focus-visible:ring-accent \
                       group-hover:opacity-100"
                class=("opacity-100", move || active.get())
                on:click=move |ev| {
                    ev.stop_propagation();
                    if let Err(error) = host.close_pane(id) {
                        leptos::logging::warn!("[reader] pane close refused: {error:?}");
                    }
                }
            >
                <Icon name=IconName::Close size=12 />
            </button>
        </div>
    }
}
