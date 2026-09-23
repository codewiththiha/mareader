//! The page-lifetime boundary between the shelf and the reader.
//!
//! Trunk's host glue cannot be released, so the boot script never lets it
//! evaluate. The shelf and the reader chrome are separate artifacts the boot
//! script instantiates. Opening a book releases the shelf instance and starts
//! a new host. Closing releases every book module and that host, then starts
//! a new shelf instance. The URL changes with the history API. A document
//! navigation does not drop the previous wasm in this webview.
//!
//! The session is a query on `/`, not the path `/reader`. Tauri's asset
//! protocol does not SPA-fallback, so a full GET of `/reader` can 404.

use std::cell::{Cell, RefCell};

use leptos::prelude::*;

use crate::state::AppState;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

thread_local! {
    /// Set while the reader page is applying the handoff it just booted with.
    /// The open path hands off whenever a boot object is present; without this
    /// the apply would navigate again and the reader would reload forever.
    static APPLYING: Cell<bool> = const { Cell::new(false) };
    /// Set for the life of a format mount. The format instance opens in this
    /// heap. It must not see the host's boot object and navigate again.
    static IN_FORMAT: Cell<bool> = const { Cell::new(false) };
    static HANDOFF_APPLIED: Cell<bool> = const { Cell::new(false) };
    /// The resume the host put in the mount payload. `open_at` reads this
    /// before the library, which a format heap does not hold.
    static RESUME: Cell<Option<(u32, Option<f64>)>> = const { Cell::new(None) };
    static DISPLAY_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// A newer open cancels a waiter that was holding the shelf for an import.
    static LEAVE_GEN: Cell<u64> = const { Cell::new(0) };
}

/// Whether this page was started by the boot script. Host tests have no
/// window and no script, and they keep the in-process open path.
pub fn boot_present() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        boot_object().is_some()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// True only for a reader boot that actually has a handoff. The script never
/// starts wasm as a reader otherwise.
pub fn is_reader() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        boot_object()
            .as_ref()
            .and_then(|boot| js_string(boot, "kind"))
            .as_deref()
            == Some("reader")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// Open and close leave the page, except while a handoff is being applied,
/// except inside a format instance, and except in a host test, which has no
/// boot object.
pub fn should_swap() -> bool {
    !in_format() && boot_present() && !APPLYING.with(|flag| flag.get())
}

/// True while this heap is a format instance. The host's copy stays false.
pub fn in_format() -> bool {
    IN_FORMAT.with(|flag| flag.get())
}

#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub fn set_in_format(on: bool) {
    IN_FORMAT.with(|flag| flag.set(on));
}

/// Park the payload's resume so the open does not ask an empty library.
#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub fn set_resume_override(page: u32, fraction: Option<f64>) {
    RESUME.with(|slot| slot.set(Some((page, fraction))));
}

pub fn take_resume_override() -> Option<(u32, Option<f64>)> {
    RESUME.with(|slot| slot.take())
}

/// The library's name for the row, when the document's own title is not one.
/// A format heap does not hold the row.
#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub fn set_display_name(name: Option<String>) {
    DISPLAY_NAME.with(|slot| *slot.borrow_mut() = name.filter(|n| !n.is_empty()));
}

pub fn display_name_override() -> Option<String> {
    DISPLAY_NAME.with(|slot| slot.borrow().clone())
}

/// Write everything the next page will load, then leave. Debounced timers die
/// with the document, so the writes here are immediate.
pub fn flush_durable(state: AppState) {
    crate::effects::appearance::flush_appearance_commit();
    let settings = state.settings.get_untracked();
    if let Err(error) = crate::storage::save_settings(&settings) {
        error.report();
    }
    crate::services::document::flush_read_point(state);
    crate::storage::persist_library(state.library);
    crate::storage::persist_covers(state.library);
}

/// Shelf to reader, or reader to a fresh reader. Does not open in this heap.
pub fn leave_for_reader(state: AppState, path: String, book_id: Option<String>) {
    #[cfg(target_arch = "wasm32")]
    {
        // Flush before status leaves Ready. `flush_read_point` no-ops unless
        // the document is Ready, and a second open must not lose the page the
        // progress effect has not written yet.
        flush_durable(state);
        state
            .reader
            .document
            .status
            .set(reader_core::DocStatus::Opening);
        state.reader.document.path.set(Some(path.clone()));
        state.reader.document.book_id.set(book_id.clone());
        // Already the reader page: drop the format instance and mount the
        // next book into the same slot. The chrome stays. The import waiter
        // is the shelf's problem; this page has already loaded.
        if is_reader() {
            crate::slot::mount_format(state, path, book_id);
            return;
        }
        let ticket = bump_leave();
        if imports_busy(state) {
            defer(state, ticket, path, book_id);
            return;
        }
        let _ = enter_reader(&path, book_id.as_deref());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (state, path, book_id);
    }
}

/// Reader to shelf. The script drops every book module and the reader host,
/// then starts a new shelf instance in this page. A document load does not
/// drop the previous wasm in this webview.
pub fn enter_library() {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = call("enterLibrary", None);
    }
}

/// Drop a payload Reload Window must not resume. The URL park is still
/// `app_chrome`'s; this only clears the key a failed park would otherwise
/// honour.
pub fn clear_handoff() {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = call("clearHandoff", None);
    }
}

/// Register the pagehide flush on the boot object. The script looks the
/// method up when the event fires, so replacing it here is enough.
pub fn install_flush(state: AppState) {
    #[cfg(target_arch = "wasm32")]
    {
        let Some(boot) = boot_object() else {
            return;
        };
        let closure = wasm_bindgen::closure::Closure::wrap(
            Box::new(move || flush_durable(state)) as Box<dyn FnMut()>
        );
        let _ = js_sys::Reflect::set(&boot, &"flush".into(), closure.as_ref());
        closure.forget();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = state;
    }
}

/// Load pdf.js into the format instance if it is not already there. The shelf
/// does not call this. A cover render used to, and that left the bridge in
/// the library heap.
pub async fn ensure_pdf_engine() {
    #[cfg(all(feature = "format-pdf", target_arch = "wasm32"))]
    {
        if pdf_engine::has_pdf_reader() {
            return;
        }
        let Some(boot) = boot_object() else {
            return;
        };
        let Ok(fun) = js_sys::Reflect::get(&boot, &"ensureEngine".into()) else {
            return;
        };
        let Ok(fun) = fun.dyn_into::<js_sys::Function>() else {
            return;
        };
        let Ok(ret) = fun.call0(&boot) else {
            return;
        };
        if let Ok(promise) = ret.dyn_into::<js_sys::Promise>() {
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        }
    }
}

/// The reader page's first act: take the handoff and mount its format
/// instance. This heap does not open the file. Missing payload means the
/// boot script should already have left; if it didn't, leave now rather
/// than sit on an empty reader. Once: the effect that calls this has no
/// signal reads, and a second apply would mount the book twice.
pub fn apply_handoff(state: AppState) {
    if HANDOFF_APPLIED.with(|flag| flag.replace(true)) {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    {
        let Some(open) = take_open() else {
            enter_library();
            return;
        };
        state
            .reader
            .document
            .status
            .set(reader_core::DocStatus::Opening);
        state.reader.document.path.set(Some(open.path.clone()));
        state.reader.document.book_id.set(open.book_id.clone());
        crate::slot::mount_format(state, open.path, open.book_id);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = state;
    }
}

#[cfg(target_arch = "wasm32")]
fn boot_object() -> Option<wasm_bindgen::JsValue> {
    let win = web_sys::window()?;
    let boot = js_sys::Reflect::get(&win, &"__MAREADER_BOOT".into()).ok()?;
    if boot.is_undefined() || boot.is_null() {
        None
    } else {
        Some(boot)
    }
}

#[cfg(target_arch = "wasm32")]
fn js_string(obj: &wasm_bindgen::JsValue, key: &str) -> Option<String> {
    let value = js_sys::Reflect::get(obj, &key.into()).ok()?;
    if value.is_undefined() || value.is_null() {
        return None;
    }
    value.as_string().filter(|s| !s.is_empty())
}

#[cfg(target_arch = "wasm32")]
fn call(method: &str, arg: Option<&wasm_bindgen::JsValue>) -> bool {
    let Some(boot) = boot_object() else {
        return false;
    };
    let Ok(fun) = js_sys::Reflect::get(&boot, &method.into()) else {
        return false;
    };
    let Ok(fun) = fun.dyn_into::<js_sys::Function>() else {
        return false;
    };
    let result = match arg {
        Some(arg) => fun.call1(&boot, arg),
        None => fun.call0(&boot),
    };
    result.is_ok()
}

#[cfg(target_arch = "wasm32")]
fn enter_reader(path: &str, book_id: Option<&str>) -> bool {
    let obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&obj, &"path".into(), &path.into());
    if let Some(id) = book_id {
        let _ = js_sys::Reflect::set(&obj, &"bookId".into(), &id.into());
    }
    call("enterReader", Some(&obj))
}

#[cfg(target_arch = "wasm32")]
fn flush_and_go(state: AppState, path: &str, book_id: Option<&str>) {
    flush_durable(state);
    let _ = enter_reader(path, book_id);
}

#[cfg(target_arch = "wasm32")]
fn imports_busy(state: AppState) -> bool {
    state.library.tasks.with_untracked(|tasks| {
        tasks
            .iter()
            .any(|task| !task.phase.is_finished() || task.waiting > 0)
    })
}

#[cfg(target_arch = "wasm32")]
fn bump_leave() -> u64 {
    LEAVE_GEN.with(|ticket| {
        let next = ticket.get().wrapping_add(1);
        ticket.set(next);
        next
    })
}

#[cfg(target_arch = "wasm32")]
fn defer(state: AppState, ticket: u64, path: String, book_id: Option<String>) {
    wasm_bindgen_futures::spawn_local(async move {
        loop {
            if LEAVE_GEN.with(|current| current.get()) != ticket {
                return;
            }
            if !imports_busy(state) {
                break;
            }
            sleep_ms(200).await;
        }
        if LEAVE_GEN.with(|current| current.get()) != ticket {
            return;
        }
        flush_and_go(state, &path, book_id.as_deref());
    });
}

#[cfg(target_arch = "wasm32")]
async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let Some(win) = web_sys::window() else {
            let _ = resolve.call0(&wasm_bindgen::JsValue::UNDEFINED);
            return;
        };
        let resolve_for_cb = resolve.clone();
        let cb = wasm_bindgen::closure::Closure::once(move || {
            let _ = resolve_for_cb.call0(&wasm_bindgen::JsValue::UNDEFINED);
        });
        let function: &js_sys::Function = cb.as_ref().unchecked_ref();
        if win
            .set_timeout_with_callback_and_timeout_and_arguments_0(function, ms)
            .is_err()
        {
            let _ = resolve.call0(&wasm_bindgen::JsValue::UNDEFINED);
        } else {
            cb.forget();
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

#[cfg(target_arch = "wasm32")]
struct OpenRequest {
    path: String,
    book_id: Option<String>,
}

#[cfg(target_arch = "wasm32")]
fn take_open() -> Option<OpenRequest> {
    let boot = boot_object()?;
    let fun = js_sys::Reflect::get(&boot, &"takeOpen".into()).ok()?;
    let fun = fun.dyn_into::<js_sys::Function>().ok()?;
    let value = fun.call0(&boot).ok()?;
    if value.is_null() || value.is_undefined() {
        return None;
    }
    Some(OpenRequest {
        path: js_string(&value, "path")?,
        book_id: js_string(&value, "bookId"),
    })
}
