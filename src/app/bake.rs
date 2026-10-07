//! The Shell's cover baker: shelf covers rendered in the Shell's own
//! frame.

use std::cell::RefCell;
use std::collections::VecDeque;

use runtime_contract::covers::{COVER_WIDTH, CoverImage};
use runtime_contract::protocol::ShellFrame;
use serde::{Deserialize, Serialize};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::app::frame::FrameKind;

/// The bake page, shipped to the dist root by the Shell's build.
const PAGE: &str = "/bake.html";
/// From the frame's insertion to its `bake-ready`: a page load plus pdf.js.
const READY_TIMEOUT_MS: i32 = 20_000;
/// One cover: a file read, a parse, a page render and a JPEG encode.
const BAKE_TIMEOUT_MS: i32 = 30_000;
/// How long an idle page stays after its queue drained.
const IDLE_TEARDOWN_MS: i32 = 5_000;

/// One shelf cover: which library frame asked, for which file.
#[derive(Clone, Debug, PartialEq, Eq)]
struct BakeRequest {
    library: u64,
    path: String,
}

/// The mounted page and everything JS-side that belongs to it.
struct Page {
    /// The Library generation that currently owns this page's work/idle grace.
    library: u64,
    iframe: web_sys::HtmlIFrameElement,
    /// The window `message` listener, removed at teardown.
    listener: Closure<dyn FnMut(web_sys::MessageEvent)>,
    ready: bool,
    ready_timer: Option<i32>,
    /// Which mount this is, so a stale timeout acts on nothing.
    epoch: u64,
}

struct InFlight {
    id: u64,
    request: BakeRequest,
    timer: Option<i32>,
}

#[derive(Default)]
struct Baker {
    queue: VecDeque<BakeRequest>,
    in_flight: Option<InFlight>,
    page: Option<Page>,
    idle_timer: Option<i32>,
    next_id: u64,
    next_epoch: u64,
    /// Answers delivered to a shelf (success or failure), for the probe.
    answered: u64,
}

impl Baker {
    /// Pure ownership transition; DOM/timer release follows outside the borrow.
    fn forget_library(&mut self, generation: u64) -> (Option<InFlight>, bool) {
        self.queue.retain(|request| request.library != generation);
        let owned = self
            .in_flight
            .as_ref()
            .is_some_and(|flight| flight.request.library == generation);
        let flight = if owned { self.in_flight.take() } else { None };
        let remove_page = self
            .page
            .as_ref()
            .is_some_and(|page| page.library == generation);
        (flight, remove_page)
    }
}

thread_local! {
    /// The one baker, page-thread state like the frame registry.
    static BAKER: RefCell<Baker> = RefCell::new(Baker::default());
}

/// The Shell → page ask.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Ask<'a> {
    kind: &'static str,
    id: u64,
    path: &'a str,
    width: f64,
}

/// Either page to Shell message; `kind` says which one.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageMessage {
    kind: String,
    #[serde(default)]
    id: u64,
    #[serde(default)]
    path: String,
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    data_url: Option<String>,
    #[serde(default)]
    width: Option<f64>,
    #[serde(default)]
    height: Option<f64>,
    #[serde(default)]
    error: Option<String>,
}

/// A library frame's ask: bake `path` for shelf frame `library`.
pub fn request(library: u64, path: String) {
    let request = BakeRequest { library, path };
    let queued = BAKER.with(|baker| {
        let mut baker = baker.borrow_mut();
        let busy = baker
            .in_flight
            .as_ref()
            .is_some_and(|flight| flight.request == request);
        if busy || baker.queue.contains(&request) {
            return false;
        }
        baker.queue.push_back(request);
        true
    });
    if queued {
        pump();
    }
}

/// A frame was admitted to the registry.
pub fn frame_registered(_generation: u64) {
    pump();
}

/// Cancel every cover owned by a Library leaving the active route.
pub fn cancel_library(generation: u64) {
    let (flight, remove_page) = BAKER.with(|baker| baker.borrow_mut().forget_library(generation));
    if let Some(flight) = flight {
        clear_timeout(flight.timer);
    }
    if remove_page {
        teardown_page();
    }
    pump();
}

/// Whether the bake page is in the document right now.
#[cfg(target_arch = "wasm32")]
pub fn resident() -> bool {
    BAKER.with(|baker| baker.borrow().page.is_some())
}

/// Answers delivered to shelves so far (the probe's `coversAnswered`).
#[cfg(target_arch = "wasm32")]
pub fn answered() -> u64 {
    BAKER.with(|baker| baker.borrow().answered)
}

/// Where the page is, from the pump's point of view.
enum PageState {
    None,
    Booting,
    Ready(web_sys::Window),
}

fn page_state() -> PageState {
    BAKER.with(|baker| {
        let baker = baker.borrow();
        match baker.page.as_ref() {
            None => PageState::None,
            Some(page) if !page.ready => PageState::Booting,
            Some(page) => match page.iframe.content_window() {
                Some(window) => PageState::Ready(window),
                None => PageState::Booting,
            },
        }
    })
}

/// Start the next queued bake if the page can take it.
fn pump() {
    if BAKER.with(|baker| baker.borrow().in_flight.is_some()) {
        return;
    }
    let next = BAKER.with(|baker| baker.borrow_mut().queue.pop_front());
    let Some(request) = next else {
        schedule_idle_teardown();
        return;
    };
    cancel_idle_teardown();
    // The shelf is not admitted yet: its bake waits for registration.
    if crate::app::frame::lookup(request.library).is_none() {
        if matches!(page_state(), PageState::None) {
            mount_page(request.library);
        }
        requeue_front(request);
        return;
    }
    match page_state() {
        PageState::None => {
            mount_page(request.library);
            requeue_front(request);
        }
        PageState::Booting => requeue_front(request),
        PageState::Ready(window) => send(window, request),
    }
}

fn requeue_front(request: BakeRequest) {
    BAKER.with(|baker| baker.borrow_mut().queue.push_front(request));
}

fn send(window: web_sys::Window, request: BakeRequest) {
    let id = BAKER.with(|baker| {
        let mut baker = baker.borrow_mut();
        if let Some(page) = baker.page.as_mut() {
            page.library = request.library;
        }
        baker.next_id += 1;
        baker.next_id
    });
    let ask = serde_wasm_bindgen::to_value(&Ask {
        kind: "mareader.bake",
        id,
        path: &request.path,
        width: COVER_WIDTH,
    });
    let posted = ask
        .ok()
        .is_some_and(|ask| window.post_message(&ask, &own_origin()).is_ok());
    if !posted {
        // A page whose window refuses a message will not answer.
        teardown_page();
        deliver(&request, None);
        pump();
        return;
    }
    let timer = set_timeout(BAKE_TIMEOUT_MS, move || on_bake_timeout(id));
    BAKER.with(|baker| {
        baker.borrow_mut().in_flight = Some(InFlight { id, request, timer });
    });
}

/// Hand an answer to the shelf that asked, if it is still there.
fn deliver(request: &BakeRequest, image: Option<CoverImage>) {
    BAKER.with(|baker| baker.borrow_mut().answered += 1);
    if let Some(library) = crate::app::frame::lookup(request.library)
        && library.kind() == FrameKind::Library
    {
        library.send(&ShellFrame::CoverBaked {
            path: request.path.clone(),
            image,
        });
    }
}

/// Insert the page and start listening for it.
fn mount_page(library: u64) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(body) = document.body() else {
        return;
    };
    let Ok(element) = document.create_element("iframe") else {
        return;
    };
    let iframe: web_sys::HtmlIFrameElement = element.unchecked_into();
    iframe.set_class_name("bake-frame");
    let _ = iframe.set_attribute("title", "MAReader cover baker");
    let _ = iframe.set_attribute("aria-hidden", "true");
    let _ = iframe.set_attribute("tabindex", "-1");
    iframe.set_src(PAGE);
    let listener =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            on_message(&event)
        });
    let _ = window.add_event_listener_with_callback("message", listener.as_ref().unchecked_ref());
    // Outside `#runtime-host`: the host holds runtime frames only.
    let _ = body.append_child(iframe.as_ref());
    let epoch = BAKER.with(|baker| {
        let mut baker = baker.borrow_mut();
        baker.next_epoch += 1;
        baker.next_epoch
    });
    let ready_timer = set_timeout(READY_TIMEOUT_MS, move || on_ready_timeout(epoch));
    BAKER.with(|baker| {
        baker.borrow_mut().page = Some(Page {
            library,
            iframe,
            listener,
            ready: false,
            ready_timer,
            epoch,
        });
    });
}

/// Remove the page: its listener, its timers, its element.
fn teardown_page() {
    cancel_idle_teardown();
    let page = BAKER.with(|baker| baker.borrow_mut().page.take());
    let Some(page) = page else {
        return;
    };
    clear_timeout(page.ready_timer);
    if let Some(window) = web_sys::window() {
        let _ = window
            .remove_event_listener_with_callback("message", page.listener.as_ref().unchecked_ref());
    }
    // Abort is synchronous in the page: cancel the render task.
    if let Some(window) = page.iframe.content_window()
        && let Ok(dispose) = js_sys::Reflect::get(&window, &"__mareaderDisposeBakes".into())
        && let Ok(dispose) = dispose.dyn_into::<js_sys::Function>()
    {
        let _ = dispose.call0(&window);
    }
    page.iframe.remove();
    drop(page.listener);
}

/// A window message, heard only from the mounted page's window.
fn on_message(event: &web_sys::MessageEvent) {
    let from_page = BAKER.with(|baker| {
        let baker = baker.borrow();
        let Some(page_window) = baker
            .page
            .as_ref()
            .and_then(|page| page.iframe.content_window())
        else {
            return false;
        };
        let Some(source) = event.source() else {
            return false;
        };
        let source: &JsValue = source.as_ref();
        let page_window: &JsValue = page_window.as_ref();
        source == page_window
    });
    if !from_page {
        return;
    }
    let origin = own_origin();
    if origin != "*" && event.origin() != origin {
        return;
    }
    let Ok(message) = serde_wasm_bindgen::from_value::<PageMessage>(event.data()) else {
        return;
    };
    match message.kind.as_str() {
        "mareader.bake-ready" => {
            let timer = BAKER.with(|baker| {
                let mut baker = baker.borrow_mut();
                let page = baker.page.as_mut()?;
                page.ready = true;
                page.ready_timer.take()
            });
            clear_timeout(timer);
            pump();
        }
        "mareader.baked" => {
            let finished = BAKER.with(|baker| {
                let mut baker = baker.borrow_mut();
                let answered = baker
                    .in_flight
                    .as_ref()
                    .is_some_and(|flight| flight.id == message.id);
                if answered {
                    baker.in_flight.take()
                } else {
                    None
                }
            });
            let Some(flight) = finished else {
                return;
            };
            clear_timeout(flight.timer);
            let image = match (message.ok, message.data_url, message.width, message.height) {
                (true, Some(data_url), Some(width), Some(height)) => Some(CoverImage {
                    data_url,
                    width,
                    height,
                }),
                _ => {
                    web_sys::console::debug_1(&JsValue::from_str(&format!(
                        "[mareader] cover bake failed for {}: {}",
                        message.path,
                        message.error.as_deref().unwrap_or("no reason reported")
                    )));
                    None
                }
            };
            deliver(&flight.request, image);
            pump();
        }
        _ => {}
    }
}

/// The page took too long over one cover: it may be wedged.
fn on_bake_timeout(id: u64) {
    let lost = BAKER.with(|baker| {
        let mut baker = baker.borrow_mut();
        let owned = baker
            .in_flight
            .as_ref()
            .is_some_and(|flight| flight.id == id);
        if owned { baker.in_flight.take() } else { None }
    });
    let Some(flight) = lost else {
        return;
    };
    web_sys::console::warn_1(&JsValue::from_str(&format!(
        "[mareader] the cover bake of {} did not answer within {} s — bake page replaced",
        flight.request.path,
        BAKE_TIMEOUT_MS / 1000
    )));
    teardown_page();
    deliver(&flight.request, None);
    pump();
}

/// The page never said ready: it is removed and covers fail.
fn on_ready_timeout(epoch: u64) {
    let stalled = BAKER.with(|baker| {
        baker
            .borrow()
            .page
            .as_ref()
            .is_some_and(|page| page.epoch == epoch && !page.ready)
    });
    if !stalled {
        return;
    }
    web_sys::console::warn_1(&JsValue::from_str(&format!(
        "[mareader] the cover bake page ({PAGE}) did not become ready within {} s — \
         its covers are answered as failures",
        READY_TIMEOUT_MS / 1000
    )));
    teardown_page();
    let waiting: Vec<BakeRequest> = BAKER.with(|baker| {
        let mut baker = baker.borrow_mut();
        let mut waiting: Vec<BakeRequest> = baker.queue.drain(..).collect();
        if let Some(flight) = baker.in_flight.take() {
            clear_timeout(flight.timer);
            waiting.push(flight.request);
        }
        waiting
    });
    for request in &waiting {
        deliver(request, None);
    }
}

/// The queue ran dry: keep the page a moment.
fn schedule_idle_teardown() {
    let armed = BAKER.with(|baker| {
        let baker = baker.borrow();
        baker.page.is_none() || baker.idle_timer.is_some()
    });
    if armed {
        return;
    }
    let timer = set_timeout(IDLE_TEARDOWN_MS, on_idle);
    BAKER.with(|baker| baker.borrow_mut().idle_timer = timer);
}

fn cancel_idle_teardown() {
    let timer = BAKER.with(|baker| baker.borrow_mut().idle_timer.take());
    clear_timeout(timer);
}

fn on_idle() {
    let idle = BAKER.with(|baker| {
        let mut baker = baker.borrow_mut();
        baker.idle_timer = None;
        baker.queue.is_empty() && baker.in_flight.is_none()
    });
    if idle {
        teardown_page();
    }
}

/// The origin every post names and every answer must carry.
fn own_origin() -> String {
    web_sys::window()
        .and_then(|window| window.location().origin().ok())
        .filter(|origin| !origin.is_empty() && origin != "null")
        .unwrap_or_else(|| "*".to_string())
}

fn set_timeout(ms: i32, f: impl FnOnce() + 'static) -> Option<i32> {
    let window = web_sys::window()?;
    let callback = Closure::once_into_js(f);
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), ms)
        .ok()
}

fn clear_timeout(id: Option<i32>) {
    if let (Some(id), Some(window)) = (id, web_sys::window()) {
        window.clear_timeout_with_handle(id);
    }
}

#[cfg(test)]
mod tests {
    use super::{BakeRequest, Baker, InFlight};

    fn request(library: u64) -> BakeRequest {
        BakeRequest {
            library,
            path: format!("/{library}.pdf"),
        }
    }

    #[test]
    fn retiring_library_cancels_its_queue_and_in_flight_without_touching_successor() {
        let mut baker = Baker {
            queue: [request(1), request(2), request(1)].into(),
            in_flight: Some(InFlight {
                id: 7,
                request: request(1),
                timer: None,
            }),
            ..Baker::default()
        };
        let (cancelled, remove_page) = baker.forget_library(1);
        assert_eq!(cancelled.unwrap().id, 7);
        assert!(!remove_page);
        assert!(baker.in_flight.is_none());
        assert_eq!(
            baker.queue.into_iter().collect::<Vec<_>>(),
            vec![request(2)]
        );
    }

    #[test]
    fn repeated_or_unrelated_retirement_does_not_cancel_another_library() {
        let mut baker = Baker {
            queue: [request(1), request(2)].into(),
            in_flight: Some(InFlight {
                id: 9,
                request: request(2),
                timer: None,
            }),
            ..Baker::default()
        };
        assert!(baker.forget_library(1).0.is_none());
        assert!(baker.forget_library(1).0.is_none());
        assert_eq!(baker.in_flight.as_ref().unwrap().id, 9);
        assert_eq!(baker.queue.front(), Some(&request(2)));
    }
}
