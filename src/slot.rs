//! The host's side of the format bridge.
//!
//! The format instance lives in another heap. What crosses is a JSON snapshot
//! up and a JSON command down, on `window.__MAREADER_SLOT`. A Leptos component
//! does not close over the loader. This module holds the copied signals the
//! chrome already reads, and nothing that is a document.

use std::cell::{Cell, RefCell};
#[cfg(target_arch = "wasm32")]
use std::sync::Arc;

use leptos::prelude::*;
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;

use crate::state::AppState;
use pdf_engine::types::DocStatus;

struct PageRect {
    left: RwSignal<f64>,
    width: RwSignal<f64>,
}

thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
    #[cfg(any(not(format_runtime), target_arch = "wasm32"))]
    static SEARCH_VISIBLE: Cell<bool> = const { Cell::new(false) };
    static FRACTION: RefCell<Option<RwSignal<Option<f64>>>> = const { RefCell::new(None) };
    static RECT: RefCell<Option<PageRect>> = const { RefCell::new(None) };
}

/// The host has asked the bootloader to mount a format instance. Document
/// keys and menu commands go across the bridge instead of into this heap.
pub fn active() -> bool {
    ACTIVE.with(|flag| flag.get())
}

/// The format instance's search bar, copied. Escape uses it so a dismiss does
/// not also close the rail.
#[cfg(not(format_runtime))]
pub fn search_visible() -> bool {
    SEARCH_VISIBLE.with(|flag| flag.get())
}

/// The stream fraction the last snapshot reported. Untracked: a flush reads
/// it outside an effect. `None` until a snapshot arrives, and for a paged book.
pub fn fraction_now() -> Option<f64> {
    FRACTION.with(|cell| cell.borrow().as_ref().and_then(|signal| signal.get_untracked()))
}

/// The page rect the floating title used to measure by walking a page-host id.
/// `(left, width)` in viewport CSS pixels. Width `0` means there is no page yet.
pub fn page_geometry() -> (f64, f64) {
    RECT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|rect| (rect.left.get(), rect.width.get()))
            .unwrap_or((0.0, 0.0))
    })
}

/// Send one command if a format instance is up. `false` means the caller
/// should apply it in this heap (a host test, or the shelf).
pub fn post(json: &str) -> bool {
    if !active() {
        return false;
    }
    #[cfg(target_arch = "wasm32")]
    {
        call_slot("command", json);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = json;
    }
    true
}

pub fn post_zoom(dir: i32) -> bool {
    post(&format!(r#"{{"op":"zoom","dir":{dir}}}"#))
}

pub fn post_mode(mode: ViewMode) -> bool {
    post(&format!(r#"{{"op":"mode","mode":"{}"}}"#, mode_token(mode)))
}

pub fn post_fit(fit: FitMode) -> bool {
    let token = match fit {
        FitMode::None => "none",
        FitMode::Width => "width",
        FitMode::Page => "page",
    };
    post(&format!(r#"{{"op":"fit","fit":"{token}"}}"#))
}

pub fn post_page(page: u32) -> bool {
    post(&format!(r#"{{"op":"page","page":{page}}}"#))
}

#[cfg(not(format_runtime))]
pub fn post_search(on: bool) -> bool {
    post(&format!(r#"{{"op":"search","on":{on}}}"#))
}

pub fn post_auto(on: bool) -> bool {
    post(&format!(r#"{{"op":"auto","on":{on}}}"#))
}

pub fn post_menu(on: bool) -> bool {
    post(&format!(r#"{{"op":"menu","on":{on}}}"#))
}

pub fn post_theme() -> bool {
    post(r#"{"op":"theme"}"#)
}

pub fn post_scrub(on: bool) -> bool {
    post(&format!(r#"{{"op":"scrub","on":{on}}}"#))
}

/// Ask the bootloader to drop whatever instance is up and mount the one this
/// path names. The host does not open the file.
#[cfg(target_arch = "wasm32")]
pub fn mount_format(state: AppState, path: String, book_id: Option<String>) {
    ACTIVE.with(|flag| flag.set(true));
    let (page, fraction) = state.library.books.with_untracked(|books| {
        library_core::book::resume_point(books, book_id.as_deref(), &path)
    });
    let display_name = book_id.as_deref().and_then(|id| {
        state.library.row(id).map(|row| row.display_name())
    });
    let payload = serde_json::json!({
        "path": path,
        "bookId": book_id,
        "page": page,
        "fraction": fraction,
        "displayName": display_name,
    });
    #[cfg(target_arch = "wasm32")]
    {
        call_boot("mountFormat", &payload.to_string());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = payload;
    }
}

/// Register the snapshot listener and the progress writer. Called once from
/// the reader session, before the first mount, so a report is not lost.
pub fn install(state: AppState) {
    if INSTALLED.with(|flag| flag.replace(true)) {
        return;
    }
    RECT.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = Some(PageRect {
                left: RwSignal::new(0.0),
                width: RwSignal::new(0.0),
            });
        }
    });
    FRACTION.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(RwSignal::new(None));
        }
    });
    install_progress(state);
    #[cfg(target_arch = "wasm32")]
    install_report(state);
}

fn install_progress(state: AppState) {
    let timer = StoredValue::new_local(None::<TimeoutHandle>);
    Effect::new(move |_| {
        // Read every dependency before the active check. A conditional read
        // drops the subscription, and this effect is installed before the
        // first snapshot arrives.
        let status = state.reader.document.status.get();
        let path = state.reader.document.path.get();
        let page = state.reader.viewer.page.get();
        let fraction = FRACTION.with(|cell| cell.borrow().as_ref().and_then(|signal| signal.get()));
        if !active() || status != DocStatus::Ready {
            return;
        }
        let Some(path) = path else {
            return;
        };
        if page == 0 || page > state.reader.document.num_pages.get_untracked() {
            return;
        }
        let book_id = state.reader.document.book_id.get_untracked();
        let mut changed = false;
        state.library.books.update(|books| {
            let at = library_core::book::rows_for_read(books, book_id.as_deref(), &path);
            for i in at {
                let Some(book) = books.get_mut(i).and_then(library_core::book::Row::as_book_mut)
                else {
                    continue;
                };
                let page_moved = book.page != page;
                let fraction_moved = match (book.fraction, fraction) {
                    (Some(old), Some(new)) => (new - old).abs() > 0.005,
                    (None, None) => false,
                    _ => true,
                };
                if page_moved {
                    book.page = page;
                }
                if fraction_moved {
                    book.fraction = fraction;
                }
                if page_moved || fraction_moved {
                    changed = true;
                }
            }
        });
        if !changed {
            return;
        }
        if let Some(handle) = timer.get_value() {
            handle.clear();
        }
        let snapshot = state.library.snapshot();
        let handle = leptos::prelude::set_timeout_with_handle(
            move || {
                if let Err(error) = crate::storage::save_library(&snapshot) {
                    error.report();
                }
            },
            std::time::Duration::from_millis(400),
        )
        .ok();
        timer.set_value(handle);
    });
}

fn mode_token(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::Single => "single",
        ViewMode::Spread => "spread",
        ViewMode::ScrollVertical => "scroll",
        ViewMode::ScrollHorizontal => "horizontal",
    }
}

#[cfg(target_arch = "wasm32")]
fn install_report(state: AppState) {
    let Some(slot) = slot_object() else {
        return;
    };
    let closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |raw: String| {
        apply_snapshot(state, &raw);
    }) as Box<dyn FnMut(String)>);
    let _ = js_sys::Reflect::set(&slot, &"report".into(), closure.as_ref());
    closure.forget();
}

#[cfg(target_arch = "wasm32")]
fn apply_snapshot(state: AppState, raw: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return;
    };
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let num = |key: &str| value.get(key).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let float = |key: &str| value.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let flag = |key: &str| value.get(key).and_then(|v| v.as_bool()).unwrap_or(false);

    let path = text("path");
    if !path.is_empty() {
        state.reader.document.path.set(Some(path));
    }
    if let Some(id) = value.get("bookId").and_then(|v| v.as_str()) {
        if !id.is_empty() {
            state.reader.document.book_id.set(Some(id.to_string()));
        }
    }
    let title = text("title");
    state
        .reader
        .document
        .title
        .set((!title.is_empty()).then_some(title));
    state.reader.document.num_pages.set(num("pageCount").max(1));
    state.reader.viewer.page.set(num("page").max(1));
    state.reader.document.format.set(parse_format(&text("format")));
    state.reader.document.status.set(parse_status(&text("status")));
    let error = text("error");
    state
        .reader
        .document
        .error
        .set((!error.is_empty()).then_some(error));
    if let Some(mode) = parse_mode(&text("mode")) {
        state.reader.viewer.mode.set(mode);
    }
    if let Some(fit) = parse_fit(&text("fit")) {
        state.reader.viewer.fit.set(fit);
    }
    let zoom = float("zoom");
    if zoom > 0.0 {
        state.reader.viewer.zoom.display.set(zoom);
    }
    state.reader.viewer.auto_scroll.set(flag("autoScroll"));
    state.reader.viewer.first_paint.set(flag("firstPaint"));
    let search = flag("searchVisible");
    state.reader.search.visible.set(search);
    SEARCH_VISIBLE.with(|flag| flag.set(search));
    let left = float("pageLeft");
    let width = float("pageWidth");
    RECT.with(|slot| {
        if let Some(rect) = slot.borrow().as_ref() {
            rect.left.set(left);
            rect.width.set(width);
        }
    });
    if let Some(outline) = value.get("outline").and_then(|v| v.as_array()) {
        let nodes = outline
            .iter()
            .filter_map(|node| {
                Some(reader_core::outline::OutlineNode::new(
                    node.get("title")?.as_str()?,
                    node.get("page")?.as_u64()? as u32,
                    node.get("depth")?.as_u64()? as u32,
                ))
            })
            .collect();
        state.reader.document.outline.set(Arc::new(nodes));
    }
    // The host does not keep the blocks. The fraction is only the resume the
    // progress effect writes back to the library blob.
    let fraction = value.get("fraction").and_then(|v| v.as_f64());
    FRACTION.with(|cell| {
        if let Some(signal) = cell.borrow().as_ref() {
            signal.set(fraction);
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn parse_format(token: &str) -> reader_core::format::Format {
    match token {
        "text" => reader_core::format::Format::Text,
        "markdown" | "md" => reader_core::format::Format::Markdown,
        _ => reader_core::format::Format::Pdf,
    }
}

#[cfg(target_arch = "wasm32")]
fn parse_status(token: &str) -> DocStatus {
    match token {
        "opening" => DocStatus::Opening,
        "ready" => DocStatus::Ready,
        "error" => DocStatus::Error,
        _ => DocStatus::Idle,
    }
}

#[cfg(target_arch = "wasm32")]
fn parse_mode(token: &str) -> Option<ViewMode> {
    Some(match token {
        "single" => ViewMode::Single,
        "spread" => ViewMode::Spread,
        "scroll" => ViewMode::ScrollVertical,
        "horizontal" => ViewMode::ScrollHorizontal,
        _ => return None,
    })
}

#[cfg(target_arch = "wasm32")]
fn parse_fit(token: &str) -> Option<FitMode> {
    Some(match token {
        "width" => FitMode::Width,
        "page" => FitMode::Page,
        "none" => FitMode::None,
        _ => return None,
    })
}

#[cfg(target_arch = "wasm32")]
fn slot_object() -> Option<wasm_bindgen::JsValue> {
    let win = web_sys::window()?;
    let slot = js_sys::Reflect::get(&win, &"__MAREADER_SLOT".into()).ok()?;
    if slot.is_undefined() || slot.is_null() {
        None
    } else {
        Some(slot)
    }
}

#[cfg(target_arch = "wasm32")]
fn call_slot(method: &str, arg: &str) {
    use wasm_bindgen::JsCast;

    let Some(slot) = slot_object() else {
        return;
    };
    let Ok(fun) = js_sys::Reflect::get(&slot, &method.into()) else {
        return;
    };
    let Ok(fun) = fun.dyn_into::<js_sys::Function>() else {
        return;
    };
    let _ = fun.call1(&slot, &arg.into());
}

#[cfg(target_arch = "wasm32")]
fn call_boot(method: &str, arg: &str) {
    use wasm_bindgen::JsCast;

    let win = web_sys::window();
    let Some(win) = win else {
        return;
    };
    let Ok(boot) = js_sys::Reflect::get(&win, &"__MAREADER_BOOT".into()) else {
        return;
    };
    if boot.is_undefined() || boot.is_null() {
        return;
    }
    let Ok(fun) = js_sys::Reflect::get(&boot, &method.into()) else {
        return;
    };
    let Ok(fun) = fun.dyn_into::<js_sys::Function>() else {
        return;
    };
    let _ = fun.call1(&boot, &arg.into());
}


