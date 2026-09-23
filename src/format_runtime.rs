//! Mount the view into the host's slot. Compiled only into a format artifact.
//! The host bin does not enable a format feature, so this module is not in
//! that artifact. `mount_to` the slot, never `mount_to_body`.

#[cfg(target_arch = "wasm32")]
use std::any::Any;
#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;

#[cfg(target_arch = "wasm32")]
use leptos::prelude::*;
#[cfg(target_arch = "wasm32")]
use serde::Deserialize;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

#[cfg(target_arch = "wasm32")]
use reader_core::format::Format;
#[cfg(target_arch = "wasm32")]
use reader_core::view::ViewMode;
#[cfg(target_arch = "wasm32")]
use reader_core::zoom_math::FitMode;

#[cfg(target_arch = "wasm32")]
use crate::state::AppState;
#[cfg(target_arch = "wasm32")]
use pdf_engine::types::DocStatus;

#[cfg(target_arch = "wasm32")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MountPayload {
    path: String,
    #[serde(default)]
    book_id: Option<String>,
    #[serde(default)]
    page: u32,
    #[serde(default)]
    fraction: Option<f64>,
    #[serde(default)]
    display_name: Option<String>,
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    // The handle is generic over the view it mounted. Erasing it keeps the
    // drop, which is the unmount, and nothing else.
    static VIEW: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
    static THUMBS: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
    static COMMAND: RefCell<Option<wasm_bindgen::closure::Closure<dyn FnMut(String)>>> =
        const { RefCell::new(None) };
}

/// Open the payload's book into `#viewer-slot`. The bootloader has already
/// dropped the previous instance. This heap is new.
pub async fn mount(payload: String) {
    #[cfg(target_arch = "wasm32")]
    {
        wasm_mount(payload).await;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = payload;
    }
}

/// Drop the view. Synchronous, so pagehide can run it before the document
/// freezes. The PDF worker is not this call: that destroy is async, and a
/// frozen page will not finish the future. The bootloader terminates the
/// worker from JS.
pub fn detach() {
    #[cfg(target_arch = "wasm32")]
    {
        null_command();
        // Thumbs first: their cleanup reads the view's signals. Dropping the
        // view owner first would unmount those signals out from under them.
        THUMBS.with(|slot| {
            slot.borrow_mut().take();
        });
        VIEW.with(|slot| {
            slot.borrow_mut().take();
        });
        crate::boot::set_in_format(false);
    }
}

/// Flush, dispose the Leptos owner, then tear the format engine down. The
/// bootloader releases the glue's `wasm` binding after this returns. Nulling
/// exports does not free that binding.
pub async fn dispose() {
    #[cfg(target_arch = "wasm32")]
    {
        crate::memory::log_heap("format-drop");
        detach();
        #[cfg(feature = "format-pdf")]
        {
            pdf_engine::api::destroy().await;
        }
    }
}

#[cfg(target_arch = "wasm32")]
async fn wasm_mount(payload: String) {
    let Ok(open) = serde_json::from_str::<MountPayload>(&payload) else {
        web_sys::console::error_1(&"[format] mount payload is not the open envelope".into());
        return;
    };
    if open.path.is_empty() {
        return;
    }
    let Some(slot) = element("viewer-slot") else {
        web_sys::console::error_1(&"[format] #viewer-slot is not in the document".into());
        return;
    };
    console_error_panic_hook::set_once();
    crate::memory::log_heap("format-mount");
    crate::boot::set_in_format(true);
    let page = open.page.max(1);
    let fraction = open.fraction;
    let path = open.path;
    let book_id = open.book_id.filter(|id| !id.is_empty());
    let display_name = open.display_name.filter(|name| !name.is_empty());
    let handle = leptos::mount::mount_to(slot, move || {
        let state = format_state();
        provide_context(state);
        let _contexts = crate::app::bootstrap_contexts(state);
        crate::boot::set_display_name(display_name);
        crate::boot::set_resume_override(page, fraction);
        install_document(state);
        crate::services::document::open::open_named(state, book_id, path);
        install_command(state);
        install_reporter(state);
        #[cfg(feature = "format-pdf")]
        mount_thumbs(state);
        surface(state)
    });
    VIEW.with(|slot| *slot.borrow_mut() = Some(Box::new(handle)));
}

#[cfg(target_arch = "wasm32")]
fn format_state() -> AppState {
    AppState {
        settings: RwSignal::new(crate::storage::load_settings()),
        ..AppState::default()
    }
}

#[cfg(target_arch = "wasm32")]
fn install_document(state: AppState) {
    crate::effects::reader::blend_backdrop::paper_settings(state);
    crate::effects::reader::link_navigation::link_navigation(state);
    crate::effects::reader::page_selection::page_selection(state);
    crate::effects::reader::selection_tracking::selection_tracking(state);
    crate::effects::app::shortcuts::shortcuts(state.reader, || {}, state.ui.sidebar);
}

#[cfg(target_arch = "wasm32")]
fn surface(state: AppState) -> impl IntoView {
    let vs = state.reader;
    let rv = crate::features::reader::use_reader_virtualizers(vs);
    crate::effects::reader::layout_prefs::layout_prefs(
        state,
        rv.virtualizer.clone(),
        rv.h_virtualizer.clone(),
    );
    crate::effects::reader::reflow_layout::reflow_layout(state, rv.virtualizer.clone());
    crate::effects::reader::reflow_measure::install_reflow_measure(state);
    crate::effects::reader::reflow_outline::reflow_outline(state);
    crate::effects::reader::mode_change::mode_change(state);
    let actuator = crate::zoom::actuator::ZoomActuator::new(rv.virtualizer.clone(), rv.h_virtualizer.clone());
    let zoom = crate::zoom::ZoomController::new(actuator);
    zoom.drive(vs);
    crate::effects::reader::navigation_sync::navigation_sync(
        vs,
        rv.virtualizer.clone(),
        rv.h_virtualizer.clone(),
    );
    crate::effects::reader::zoom_watchers::follow_watcher(state, state.ui.sidebar);
    crate::effects::reader::zoom_watchers::fit_watcher(state);
    crate::effects::reader::auto_scroll::auto_scroll(vs);
    // After navigation_sync: both wake when a zoom transaction closes, and
    // progress must persist the page the jump replayed, not the stale one.
    crate::effects::reader::reading_progress::reading_progress(state);
    crate::effects::reader::blend_backdrop::blend_backdrop(state);
    crate::effects::reader::first_paint::first_paint_gate(state);

    let progress_visible = Signal::derive(move || state.settings.with(|st| st.layout.progress_bar));
    let show_indicator = Signal::derive(move || state.settings.with(|st| st.layout.page_indicator));
    let indicator_style = Signal::derive(move || state.settings.with(|st| st.layout.page_indicator_style));
    let stream_live = Signal::derive(move || vs.reflow_streaming());
    let stream_percent = Signal::derive(move || vs.stream_percent());
    let is_ready = move || vs.document.status.get() == DocStatus::Ready;

    view! {
        <Show when=is_ready>
            <crate::components::viewer::Viewer
                state=vs
                virtualizer=rv.virtualizer_view.get_value()
                h_virtualizer=rv.h_virtualizer_view.get_value()
                progress_visible=progress_visible
            />
        </Show>
        <Show when=move || is_ready() && !vs.viewer.first_paint.get()>
            <div
                class=format!(
                    "absolute inset-0 {} flex items-center justify-center",
                    app_chrome::layers::DRAG_OVERLAY
                )
                style=move || format!(
                    "background:{}",
                    if vs.reflowable() { "var(--tx-paper)" } else { "var(--color-paper)" }
                )
            >
                <crate::components::primitives::feedback::CenteredLoader />
            </div>
        </Show>
        <Show when=move || is_ready() && show_indicator.get()>
            <div class=format!("pointer-events-none absolute bottom-3 right-3 {}", app_chrome::layers::CONTROLS)>
                <crate::components::viewer::controls::page_indicator::PageIndicator
                    current=Signal::derive(move || {
                        if stream_live.get() { stream_percent.get() } else { vs.viewer.page.get() }
                    })
                    total=Signal::derive(move || {
                        if stream_live.get() { 100 } else { vs.document.num_pages.get() }
                    })
                    style=Signal::derive(move || {
                        if stream_live.get() {
                            reader_core::settings::PageIndicatorStyle::Percentage
                        } else {
                            indicator_style.get()
                        }
                    })
                    hidden=Signal::derive(move || state.reader.gloss.selection_active.get())
                />
            </div>
        </Show>
        <crate::components::viewer::controls::bottom_bar::ReaderBottomBar reader=vs />
        <crate::components::search::floating_search::FloatingSearch
            state=vs
            virtualizer=rv.virtualizer_view
        />
        <crate::components::ai::selection_pill::SelectionPill state=state />
        <crate::components::ai::gloss::gloss_ai_popover::GlossAiPopover state=state />
    }
}

#[cfg(target_arch = "wasm32")]
fn install_command(state: AppState) {
    let Some(slot) = slot_object() else {
        return;
    };
    let closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |raw: String| {
        apply_command(state, &raw);
    }) as Box<dyn FnMut(String)>);
    let _ = js_sys::Reflect::set(&slot, &"command".into(), closure.as_ref());
    COMMAND.with(|slot| *slot.borrow_mut() = Some(closure));
}

#[cfg(target_arch = "wasm32")]
fn null_command() {
    if let Some(slot) = slot_object() {
        let _ = js_sys::Reflect::set(&slot, &"command".into(), &wasm_bindgen::JsValue::UNDEFINED);
    }
    COMMAND.with(|slot| {
        slot.borrow_mut().take();
    });
}

#[cfg(target_arch = "wasm32")]
fn apply_command(state: AppState, raw: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return;
    };
    let op = value.get("op").and_then(|v| v.as_str()).unwrap_or("");
    match op {
        "zoom" => {
            let dir = value.get("dir").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            if dir != 0 {
                state.reader.viewer.zoom.post(crate::state::reader::ZoomCommand::Step(dir), true);
            }
        }
        "mode" => {
            if let Some(mode) = value.get("mode").and_then(|v| v.as_str()).and_then(parse_mode) {
                state.reader.viewer.mode.set(mode);
            }
        }
        "fit" => {
            if let Some(fit) = value.get("fit").and_then(|v| v.as_str()).and_then(parse_fit) {
                state.reader.viewer.fit.set(fit);
            }
        }
        "page" => {
            if let Some(page) = value.get("page").and_then(|v| v.as_u64()) {
                state.reader.viewer.page.set(page as u32);
            }
        }
        "search" => {
            let on = value.get("on").and_then(|v| v.as_bool()).unwrap_or(false);
            if on {
                crate::effects::reader::search::resume_search(state.reader);
            } else {
                crate::effects::reader::search::dismiss_search(state.reader);
            }
        }
        "auto" => {
            if let Some(on) = value.get("on").and_then(|v| v.as_bool()) {
                state.reader.viewer.auto_scroll.set(on);
            }
        }
        #[cfg(feature = "format-pdf")]
        "menu" => {
            if let Some(on) = value.get("on").and_then(|v| v.as_bool()) {
                pdf_engine::api::set_appearance_menu_open(on);
            }
        }
        #[cfg(feature = "format-pdf")]
        "theme" => pdf_engine::api::refresh_theme(),
        #[cfg(feature = "format-pdf")]
        "scrub" => {
            if let Some(on) = value.get("on").and_then(|v| v.as_bool()) {
                pdf_engine::api::set_scrub_mode(on);
            }
        }
        _ => {}
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
fn install_reporter(state: AppState) {
    Effect::new(move |_| {
        let status = state.reader.document.status.get();
        let page = state.reader.viewer.page.get();
        let page_count = state.reader.document.num_pages.get();
        let title = state.reader.document.title.get().unwrap_or_default();
        let format = state.reader.document.format.get();
        let outline = state.reader.document.outline.get();
        let mode = state.reader.viewer.mode.get();
        let fit = state.reader.viewer.fit.get();
        let zoom = state.reader.viewer.zoom.display.get();
        let auto_scroll = state.reader.viewer.auto_scroll.get();
        let first_paint = state.reader.viewer.first_paint.get();
        let search_visible = state.reader.search.visible.get();
        let error = state.reader.document.error.get().unwrap_or_default();
        let path = state.reader.document.path.get().unwrap_or_default();
        let book_id = state.reader.document.book_id.get().unwrap_or_default();
        let _scroll = state.reader.viewer.scroll_top.get();
        let fraction = state.reader.stream_fraction();
        let (page_left, page_width) = page_rect(state.reader);
        let outline = outline
            .iter()
            .map(|node| {
                serde_json::json!({
                    "title": node.title,
                    "page": node.page,
                    "depth": node.depth,
                })
            })
            .collect::<Vec<_>>();
        let snapshot = serde_json::json!({
            "path": path,
            "bookId": book_id,
            "title": title,
            "page": page,
            "pageCount": page_count,
            "format": format_token(format),
            "status": status_token(status),
            "outline": outline,
            "mode": mode_token(mode),
            "fit": fit_token(fit),
            "zoom": zoom,
            "autoScroll": auto_scroll,
            "firstPaint": first_paint,
            "searchVisible": search_visible,
            "error": error,
            "pageLeft": page_left,
            "pageWidth": page_width,
            "fraction": fraction,
        });
        call_slot("report", &snapshot.to_string());
    });
}

#[cfg(target_arch = "wasm32")]
fn format_token(format: Format) -> &'static str {
    match format {
        Format::Pdf => "pdf",
        Format::Text => "text",
        Format::Markdown => "markdown",
    }
}

#[cfg(target_arch = "wasm32")]
fn status_token(status: DocStatus) -> &'static str {
    match status {
        DocStatus::Idle => "idle",
        DocStatus::Opening => "opening",
        DocStatus::Ready => "ready",
        DocStatus::Error => "error",
    }
}

#[cfg(target_arch = "wasm32")]
fn mode_token(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::Single => "single",
        ViewMode::Spread => "spread",
        ViewMode::ScrollVertical => "scroll",
        ViewMode::ScrollHorizontal => "horizontal",
    }
}

#[cfg(target_arch = "wasm32")]
fn fit_token(fit: FitMode) -> &'static str {
    match fit {
        FitMode::None => "none",
        FitMode::Width => "width",
        FitMode::Page => "page",
    }
}

#[cfg(target_arch = "wasm32")]
fn page_rect(state: crate::state::ReaderState) -> (f64, f64) {
    let page = state.viewer.page.get_untracked().max(1);
    let mode = state.viewer.mode.get_untracked();
    let id = crate::components::viewer::page_host::host_id_for_mode(mode, page);
    let Some(el) = web_sys::window()
        .and_then(|win| win.document())
        .and_then(|doc| doc.get_element_by_id(&id))
    else {
        return (0.0, 0.0);
    };
    let rect = el.get_bounding_client_rect();
    (rect.left(), rect.width())
}

#[cfg(all(target_arch = "wasm32", feature = "format-pdf"))]
fn mount_thumbs(state: AppState) {
    let Some(el) = query("[data-thumb-mount]") else {
        return;
    };
    let reader = state.reader;
    let sidebar = state.ui.sidebar;
    let live = Signal::derive(|| true);
    let handle = leptos::mount::mount_to(el, move || {
        view! {
            <crate::components::shell::sidebar::panels::thumbnails::panel::ThumbnailsPanel
                state=reader
                live=live
                sidebar=sidebar
            />
        }
    });
    THUMBS.with(|slot| *slot.borrow_mut() = Some(Box::new(handle)));
}

#[cfg(target_arch = "wasm32")]
fn element(id: &str) -> Option<web_sys::HtmlElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id(id)?
        .dyn_into()
        .ok()
}

#[cfg(all(target_arch = "wasm32", feature = "format-pdf"))]
fn query(selector: &str) -> Option<web_sys::HtmlElement> {
    web_sys::window()?
        .document()?
        .query_selector(selector)
        .ok()
        .flatten()?
        .dyn_into()
        .ok()
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
