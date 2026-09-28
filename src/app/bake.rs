//! The Shell's cover baker: shelf covers rendered in a frame of the Shell's
//! own, so the library never needs a reader for them.
//!
//! A shelf cover is page 1 of a PDF as a small JPEG. The library artifact
//! carries no PDF code (`docs/runtime-split.md`, the dependency gate), so it
//! asks the Shell — `ShellApi::bake_cover`, `RuntimeFrame::BakeCover` on the
//! wire — and the Shell used to relay the ask to a READER frame. That relay
//! was the second reason (after warming) a reader had to stay booted behind
//! the shelf, and a route back to the library could not leave only the
//! library resident. This module replaces it: a hidden `public/bake.html`
//! (its script is `public/coverBake.ts`) that loads pdf.js and the engine's
//! cover render — no wasm, no runtime, no session — is mounted when the
//! first bake is queued, drains the queue one file at a time, and is removed
//! a few seconds after the queue runs dry. The Shell page itself still loads
//! no engine: the bake page is a child document, and it lives exactly as
//! long as there is work.
//!
//! The wire is three same-origin window messages (the page's header spells
//! them): `bake-ready` from the page once its script runs, `bake` from the
//! Shell per cover, `baked` back per ask. Answers are accepted only from the
//! frame this module mounted — the event's source is compared to that
//! frame's window — and only on the Shell's own origin.
//!
//! Every wait is bounded (§6): the page has [`READY_TIMEOUT_MS`] to say
//! ready and each bake has [`BAKE_TIMEOUT_MS`]; a page that misses either is
//! removed, the cover it owed is answered as a failure (the shelf's
//! one-retry policy asks again), and the next ask mounts a fresh page.
//!
//! The asks are keyed on the shelf's frame generation, and a cold shelf
//! asks from inside its own mount — before its Ready verdict admits it to
//! the frame registry. Such an ask waits for the admission
//! ([`frame_registered`]) while the page already boots, so the wait costs
//! the shelf nothing; an ask whose frame is torn down instead — a boot
//! that failed, a shelf that was replaced — is pruned ([`frame_gone`]):
//! nobody is left to show the cover, and the next shelf asks again for what
//! it lacks.

use std::cell::RefCell;
use std::collections::VecDeque;

use runtime_contract::covers::{COVER_WIDTH, CoverImage};
use runtime_contract::protocol::ShellFrame;
use serde::{Deserialize, Serialize};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::app::frame::FrameKind;

/// The bake page, shipped to the dist root by the Shell's own build
/// (`index.html` copies it beside `coverBake.js`; the artifact contract in
/// `tools/check-runtime-artifacts.mjs` pins both).
const PAGE: &str = "/bake.html";
/// From the frame's insertion to its `bake-ready`: a page load plus pdf.js.
const READY_TIMEOUT_MS: i32 = 20_000;
/// One cover: a file read, a parse, a page render and a JPEG encode.
const BAKE_TIMEOUT_MS: i32 = 30_000;
/// How long an idle page stays after its queue drained. A shelf that is
/// still importing asks in bursts; a page kept for a few seconds serves the
/// next burst without another load, and one that stays quiet is removed.
const IDLE_TEARDOWN_MS: i32 = 5_000;

/// One shelf cover: which library frame asked, for which file.
#[derive(Clone, Debug, PartialEq, Eq)]
struct BakeRequest {
    library: u64,
    path: String,
}

/// The mounted page and everything JS-side that belongs to it.
struct Page {
    iframe: web_sys::HtmlIFrameElement,
    /// The window `message` listener. Removed explicitly at teardown; kept
    /// here so it lives exactly as long as the page it listens for.
    listener: Closure<dyn FnMut(web_sys::MessageEvent)>,
    ready: bool,
    ready_timer: Option<i32>,
    /// Which mount this is: a ready timeout armed for one page must not act
    /// on the page that replaced it.
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

thread_local! {
    /// The one baker. Page-thread state (DOM handles, closures), like the
    /// frame registry: the manager only ever calls in from this thread.
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

/// Either page → Shell message; the fields an answer does not carry stay
/// at their defaults, and `kind` says which message this is.
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

/// A library frame's ask: bake `path` for the shelf in frame `library`.
/// Deduplicated against the queue and the bake in flight — a shelf that
/// asks twice for one file while it waits gets one answer.
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

/// A frame was admitted to the registry (`frame::register`). The asks a
/// shelf made from inside its mount were waiting for exactly this: the
/// pump starts them now.
pub fn frame_registered(_generation: u64) {
    pump();
}

/// A frame was torn down (`frame::unregister`). Its asks are pruned — a
/// shelf that failed to boot never gets admitted, and a shelf that was
/// replaced has nobody left to show the cover. An ask of its already in
/// flight runs to its answer, which [`deliver`] then drops at the boundary.
pub fn frame_gone(generation: u64) {
    let pruned = BAKER.with(|baker| {
        let mut baker = baker.borrow_mut();
        let before = baker.queue.len();
        baker.queue.retain(|request| request.library != generation);
        baker.queue.len() != before
    });
    if pruned {
        pump();
    }
}

/// Whether the bake page is in the document right now (the probe's
/// `bakeFrameResident`): true only while covers are being baked or for the
/// idle grace after, never at rest.
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

/// Start the next queued bake if nothing is in flight and the page can take
/// it. Called on every event that can make one possible: a request, an
/// answer, the page becoming ready, a timeout clearing the slot.
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
    // The shelf is not admitted yet (a cold shelf asks from inside its
    // mount, ahead of its Ready verdict): its bake waits for
    // `frame_registered`, but the page boots meanwhile, so the wait costs
    // nothing. A shelf that is torn down instead has its asks pruned by
    // `frame_gone`, so this never waits on a frame that is gone.
    if crate::app::frame::lookup(request.library).is_none() {
        if matches!(page_state(), PageState::None) {
            mount_page();
        }
        requeue_front(request);
        return;
    }
    match page_state() {
        PageState::None => {
            mount_page();
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
        // A page whose window refuses a message is not going to answer
        // anything: replace it, and let the shelf's retry ask again.
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

/// Hand an answer to the shelf that asked, if it is still there. A bake
/// that outlived its shelf dies at the boundary (§35): it is never misfiled
/// into a replacement session's state.
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

/// Insert the page and start listening for it. Nothing is sent until it
/// says `bake-ready`; the ready timeout bounds that wait.
fn mount_page() {
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
    let _ = iframe.set_attribute("data-mareader-bake-frame", "");
    let _ = iframe.set_attribute("aria-hidden", "true");
    let _ = iframe.set_attribute("tabindex", "-1");
    iframe.set_src(PAGE);
    let listener =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            on_message(&event)
        });
    let _ = window.add_event_listener_with_callback("message", listener.as_ref().unchecked_ref());
    // Outside `#runtime-host` on purpose: the host holds runtime frames and
    // nothing else, and the suites count what sits in it.
    let _ = body.append_child(iframe.as_ref());
    let epoch = BAKER.with(|baker| {
        let mut baker = baker.borrow_mut();
        baker.next_epoch += 1;
        baker.next_epoch
    });
    let ready_timer = set_timeout(READY_TIMEOUT_MS, move || on_ready_timeout(epoch));
    BAKER.with(|baker| {
        baker.borrow_mut().page = Some(Page {
            iframe,
            listener,
            ready: false,
            ready_timer,
            epoch,
        });
    });
}

/// Remove the page: its listener, its timers, its element. The bake it may
/// have been running is the caller's to answer.
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
    page.iframe.remove();
    drop(page.listener);
}

/// A window message. Only the mounted page's own window is heard, and only
/// on the Shell's origin; everything else on the window is not this
/// module's business.
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

/// The page took too long over one cover: it may be wedged (a worker that
/// never came up, a parse that never ends), so the page goes and the cover
/// is answered as a failure.
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

/// The page never said ready: it is removed and every waiting cover is
/// answered as a failure. The shelf asks once more for each (its retry),
/// which mounts a fresh page — so a page that cannot load fails each cover
/// exactly twice and then the queue is quiet, never a loop.
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

/// The queue ran dry: keep the page a moment for the next burst, then
/// remove it if nothing came.
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

/// The origin every post names and every answer must carry (§8: exact,
/// never `*` — except for an opaque origin, which cannot be named at all).
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
