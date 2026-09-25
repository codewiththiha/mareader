//! The runtime host's boot states: what the user sees in `#runtime-host` while
//! a runtime is loading, and what they see when one cannot start.
//!
//! Two invariants live here (§5, §6, §11):
//!
//! * the host is NEVER uncovered — it holds a loading state, an active runtime,
//!   or an error state at every moment the window is on screen (an uncovered
//!   host is a blank window). "The runtime became active" is NOT the same
//!   moment as "the runtime painted": `mount_to` clears the container it is
//!   handed, and a runtime whose first render is a suspense anchor paints no
//!   elements for a frame or more, so the shell keeps its loading card up
//!   until the runtime's own DOM is in the host (see [`watch_paint`]);
//! * a failure is VISIBLE and NAMED — runtime, stage (module load / init /
//!   start) and the underlying failure — while the console keeps the detail.
//!
//! The shell page carries its own placeholder (`#shell-boot` in index.html)
//! for the window before the shell wasm has mounted anything; it is page-owned
//! on purpose, because everything else needs a runtime to exist first. This
//! module is the second half of that: once the shell is running, the host
//! paints its own states, and the page placeholder steps aside.
//!
//! This is the ONLY legitimate fallback. If `library.js` cannot load, the old
//! LibraryPage does not come back: the user gets this error surface, which
//! names what failed and where.

use std::cell::RefCell;

use serde_json::json;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

/// Which runtime a boot state or failure is about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuntimeName {
    Library,
    Reader,
}

impl RuntimeName {
    /// The artifact stem: `library` -> `/library.js` + `/library_bg.wasm`.
    pub const fn artifact(self) -> &'static str {
        match self {
            RuntimeName::Library => "library",
            RuntimeName::Reader => "reader",
        }
    }

    /// The name as the UI says it.
    pub const fn label(self) -> &'static str {
        match self {
            RuntimeName::Library => "Library",
            RuntimeName::Reader => "Reader",
        }
    }
}

/// Where in the boot sequence a runtime failed (§6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootStage {
    /// The outgoing session is being torn down; the replacement must not
    /// become active until it is gone (§10).
    Dispose,
    /// `/library.js` or `/reader.js` did not load.
    ModuleLoad,
    /// The artifact's wasm instance did not initialize.
    Init,
    /// The `*Start` export did not start a session.
    Start,
}

impl BootStage {
    /// What the UI says (§6's stage list, in the user's words).
    pub const fn label(self) -> &'static str {
        match self {
            BootStage::Dispose => "dispose",
            BootStage::ModuleLoad => "module load",
            BootStage::Init => "init",
            BootStage::Start => "start",
        }
    }

    /// The machine-readable form (`data-mareader-stage`, diagnostics).
    pub const fn slug(self) -> &'static str {
        match self {
            BootStage::Dispose => "dispose",
            BootStage::ModuleLoad => "module-load",
            BootStage::Init => "init",
            BootStage::Start => "start",
        }
    }

    /// What the boot was doing, in the user's terms. The artifact name is part
    /// of the module-load wording because that is the step a missing file
    /// breaks, and the path is what the user can check.
    pub fn doing(self, runtime: RuntimeName) -> String {
        let name = runtime.artifact();
        match self {
            BootStage::Dispose => format!("disposing the previous {name} session"),
            BootStage::ModuleLoad => format!("loading /{name}.js"),
            BootStage::Init => format!("initializing the {name} wasm module"),
            BootStage::Start => format!("starting the {name} session"),
        }
    }
}

/// A runtime that did not start, with everything the UI needs to name it and
/// everything the console needs to explain it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BootError {
    pub runtime: RuntimeName,
    pub stage: BootStage,
    pub message: String,
}

/// The longest failure text the UI shows. The console is where the full value
/// goes; a wall of stack trace in the window is not an error message (§6).
const MAX_DETAIL: usize = 240;

impl BootError {
    pub fn new(runtime: RuntimeName, stage: BootStage, message: impl AsRef<str>) -> Self {
        // One line, bounded: the first line is where the useful part is, and a
        // wasm panic's "unreachable" plus a minified trace is not.
        let text = message.as_ref().lines().next().unwrap_or("").trim();
        let text = if text.is_empty() {
            "no further detail was reported"
        } else {
            text
        };
        let mut message = text.to_string();
        if message.chars().count() > MAX_DETAIL {
            message = message.chars().take(MAX_DETAIL).collect::<String>() + "…";
        }
        Self {
            runtime,
            stage,
            message,
        }
    }

    /// The headline for the window.
    pub fn headline(&self) -> String {
        format!(
            "MAReader could not start the {} runtime",
            self.runtime.label()
        )
    }

    /// The sub-line: which stage failed, doing what.
    pub fn context(&self) -> String {
        let doing = self.stage.doing(self.runtime);
        format!("Failed during {} — {doing}.", self.stage.label())
    }

    /// What the console gets. The runtime and stage appear in the same words
    /// the UI uses, so a report of one can be matched to the other.
    pub fn console_line(&self) -> String {
        let doing = self.stage.doing(self.runtime);
        format!("[mareader] boot failed: {doing} — {}", self.message)
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "runtime": self.runtime.artifact(),
            "stage": self.stage.slug(),
            "message": self.message,
        })
    }
}

/// The shell's boot phase, published as a signal so the page's own placeholder
/// can step aside exactly when the host starts painting — and not before.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BootPhase {
    /// The shell wasm is up; nothing has been painted into the host yet.
    Booting,
    /// The host is showing a loading state for this runtime.
    Loading(RuntimeName),
    /// A runtime is mounted and live in the host.
    Active(RuntimeName),
    /// A runtime failed; the host is showing the error state.
    Failed(BootError),
}

impl BootPhase {
    /// The stable string the diagnostics probe reports and the browser tests
    /// assert on.
    pub fn as_str(&self) -> &'static str {
        match self {
            BootPhase::Booting => "booting",
            BootPhase::Loading(RuntimeName::Library) => "loading-library",
            BootPhase::Loading(RuntimeName::Reader) => "loading-reader",
            BootPhase::Active(RuntimeName::Library) => "library",
            BootPhase::Active(RuntimeName::Reader) => "reader",
            BootPhase::Failed(_) => "failed",
        }
    }

    pub fn error(&self) -> Option<&BootError> {
        match self {
            BootPhase::Failed(error) => Some(error),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// The DOM. Everything the host paints carries `data-mareader-boot`, which is
// also how it is cleared: the cleanup removes ONLY these nodes, so it can never
// take out a mounted runtime's subtree.
// ---------------------------------------------------------------------------

const BOOT_ATTR: &str = "data-mareader-boot";
/// Set on the host itself once a runtime is mounted (the active state).
const ACTIVE_ATTR: &str = "data-mareader-active";
/// Which boot node is the shell's loading state, as opposed to the error state.
const LOADING: &str = "load";
/// How long a runtime gets to stop churning DOM before the coverage watch
/// stands down. A boot's paint order is not one event (a suspense fallback, the
/// document's own arrival, a view swap), so the host is watched across the
/// whole handover rather than at a single moment.
const WATCH_WINDOW_MS: f64 = 30_000.0;

thread_local! {
    /// The running coverage watch and the generation that owns it: a watch
    /// started for a runtime that has since been replaced must not touch the
    /// host its successor is painting into.
    static WATCH_OBSERVER: RefCell<Option<web_sys::MutationObserver>> =
        const { RefCell::new(None) };
    static WATCH_GENERATION: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static NEXT_WATCH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn document() -> Option<web_sys::Document> {
    web_sys::window().and_then(|w| w.document())
}

fn element(tag: &str) -> Option<web_sys::Element> {
    document().and_then(|d| d.create_element(tag).ok())
}

fn text_node(parent: &web_sys::Element, tag: &str, class: &str, text: &str) {
    let Some(node) = element(tag) else {
        return;
    };
    let _ = node.set_attribute("class", class);
    node.set_text_content(Some(text));
    let _ = parent.append_child(&node);
}

/// The loading card itself. Separate from painting it because the coverage
/// watch re-appends it after a runtime's `mount_to` cleared the container.
fn loading_card(runtime: RuntimeName) -> Option<web_sys::Element> {
    let card = element("div")?;
    let _ = card.set_attribute("class", "runtime-boot");
    let _ = card.set_attribute(BOOT_ATTR, LOADING);
    let _ = card.set_attribute("role", "status");
    let _ = card.set_attribute("aria-live", "polite");
    text_node(&card, "p", "runtime-boot__title", "Loading MAReader…");
    let hint = format!("Starting the {} runtime", runtime.label());
    text_node(&card, "p", "runtime-boot__hint", &hint);
    Some(card)
}

/// The shell's loading state, painted into the host before any await. A start
/// also retires the previous runtime's active marker: nothing is active while
/// the replacement is loading.
pub fn paint_loading(host: &web_sys::Element, runtime: RuntimeName) {
    let _ = host.remove_attribute(ACTIVE_ATTR);
    clear_loading(host);
    cover(host, runtime);
}

/// Put the loading state back if the host has none. Idempotent on purpose: it
/// is what every coverage tick calls, and it must never stack a second card or
/// touch anything a runtime painted.
pub fn cover(host: &web_sys::Element, runtime: RuntimeName) {
    let Ok(nodes) = host.query_selector_all(&format!("[{BOOT_ATTR}]")) else {
        return;
    };
    if nodes.length() > 0 {
        return;
    }
    if let Some(card) = loading_card(runtime) {
        let _ = host.append_child(&card);
    }
}

/// Has a runtime (or the error state) painted into the host? The shell's own
/// loading card is the one thing that does not count: it is what this question
/// is asked in order to take away.
pub fn painted(host: &web_sys::Element) -> bool {
    let mut child = host.first_element_child();
    while let Some(node) = child {
        let is_loading = node.get_attribute(BOOT_ATTR).as_deref() == Some(LOADING);
        if !is_loading {
            return true;
        }
        child = node.next_element_sibling();
    }
    false
}

/// Remove the shell's LOADING state only. `clear` takes every boot node, which
/// after a failure includes the error card — the coverage watch must never do
/// that.
pub fn clear_loading(host: &web_sys::Element) {
    let Ok(nodes) = host.query_selector_all(&format!("[{BOOT_ATTR}=\"{LOADING}\"]")) else {
        return;
    };
    for index in 0..nodes.length() {
        let Some(node) = nodes.item(index) else {
            continue;
        };
        if let Some(parent) = node.parent_node() {
            let _ = parent.remove_child(&node);
        }
    }
}

/// Remove the page's own placeholder (`#shell-boot`, index.html). The shell
/// does this — not the page — because the shell is what knows a runtime has
/// painted: uncover too early and the window is blank, too late and the
/// placeholder covers the app (public/shellBoot.js keeps the 20 s watchdog for
/// the case where the shell never runs at all).
pub fn uncover_page() {
    let Some(document) = document() else {
        return;
    };
    let Some(boot) = document.get_element_by_id("shell-boot") else {
        return;
    };
    if let Some(parent) = boot.parent_node() {
        let _ = parent.remove_child(&boot);
    }
}

/// The error state (§6). The button reloads the window: a failed dynamic
/// import is cached by the browser's module map, so re-importing in the same
/// document is not a retry — a fresh document is.
pub fn paint_error(host: &web_sys::Element, error: &BootError) {
    clear(host);
    let Some(card) = element("div") else {
        return;
    };
    let _ = card.set_attribute("class", "runtime-boot runtime-boot--error");
    let _ = card.set_attribute(BOOT_ATTR, "error");
    // The machine-readable half of the message: the browser suites assert on
    // these, and a support report can quote them.
    let _ = card.set_attribute("data-mareader-runtime", error.runtime.artifact());
    let _ = card.set_attribute("data-mareader-stage", error.stage.slug());
    let _ = card.set_attribute("role", "alert");
    text_node(&card, "p", "runtime-boot__title", &error.headline());
    text_node(&card, "p", "runtime-boot__hint", &error.context());
    text_node(&card, "p", "runtime-boot__detail", &error.message);
    let reload = js_sys::Function::new_no_args("window.location.reload()");
    if let Some(button) = retry_button(&reload, "Reload MAReader") {
        let _ = card.append_child(&button);
    }
    let _ = host.append_child(&card);
    // The console keeps the detail (§6): one line, with the artifact, the stage
    // and the actual failure the browser reported.
    web_sys::console::error_1(&JsValue::from_str(&error.console_line()));
}

fn retry_button(on_click: &js_sys::Function, label: &str) -> Option<web_sys::Element> {
    let button = element("button")?;
    let _ = button.set_attribute("class", "runtime-boot__retry");
    let _ = button.set_attribute("type", "button");
    button.set_text_content(Some(label));
    let button: web_sys::HtmlButtonElement = button.unchecked_into();
    button.set_onclick(Some(on_click));
    Some(button.unchecked_into())
}

/// Mark the host as holding a live runtime, then keep the window covered until
/// that runtime has painted. Called after the runtime's start export returned:
/// "active" is the manager's word for "the session exists", and a session can
/// exist for a frame or more before its first element is in the DOM — the
/// window is not allowed to be uncovered in between (§11).
pub fn mark_active(host: &web_sys::Element, runtime: RuntimeName) {
    let _ = host.set_attribute(ACTIVE_ATTR, runtime.artifact());
    if painted(host) {
        // Painted inside its own start call (the library does): nothing to
        // cover, and the placeholder can go in this same step.
        clear_loading(host);
        uncover_page();
    } else {
        cover(host, runtime);
    }
    // The watch goes on either way, and that is the point: what is in the host
    // at this instant is the runtime's FIRST state, not its final one. A view
    // swap that takes the old DOM out before the new is in is exactly the
    // window this exists for — checking once and standing down is how the host
    // went bare with the loading state already gone.
    watch_paint(host.clone(), runtime);
}

/// The coverage watch: keep the window covered while a runtime's DOM is not
/// there, and take the shell's card (and the page's placeholder) away the
/// moment it is.
///
/// A MutationObserver, not a timer, and that choice is the whole point: its
/// callback runs in the microtask checkpoint of the task that mutated the host,
/// so re-covering cannot be observed from another task — a timer would leave a
/// real hole of up to a tick, which is a blank window at 60 Hz.
fn watch_paint(host: web_sys::Element, runtime: RuntimeName) {
    let generation = NEXT_WATCH.with(|n| {
        let next = n.get().wrapping_add(1);
        n.set(next);
        next
    });
    // The previous watch belongs to a runtime that is already gone.
    stop_watch();
    let observed = host.clone();
    let since = js_sys::Date::now();
    let callback = Closure::<dyn FnMut()>::new(move || {
        if WATCH_GENERATION.with(|current| current.get()) != generation {
            stop_watch();
            return;
        }
        if js_sys::Date::now() - since > WATCH_WINDOW_MS {
            stop_watch();
            return;
        }
        if painted(&observed) {
            clear_loading(&observed);
            uncover_page();
            return;
        }
        cover(&observed, runtime);
    });
    let Ok(observer) = web_sys::MutationObserver::new(callback.as_ref().unchecked_ref()) else {
        // Without an observer the card painted by the caller still covers the
        // window; only the handover would be missed, so this is not fatal.
        return;
    };
    let init = web_sys::MutationObserverInit::new();
    init.set_child_list(true);
    if observer.observe_with_options(&host, &init).is_err() {
        return;
    }
    // The observer holds the JS function, and the function holds the closure:
    // `into_js_value` gives that ownership to JS, and both are collected once
    // [`stop_watch`] disconnects and drops the observer.
    drop(callback.into_js_value());
    WATCH_OBSERVER.with(|slot| *slot.borrow_mut() = Some(observer));
    WATCH_GENERATION.with(|slot| slot.set(generation));
}

/// Stop the coverage watch, if one is running.
pub fn stop_watch() {
    WATCH_OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow_mut().take() {
            observer.disconnect();
        }
    });
    WATCH_GENERATION.with(|slot| slot.set(0));
}

/// Remove the shell's boot markup. Scoped to `[data-mareader-boot]`: a mounted
/// runtime's own DOM does not carry the attribute and is never touched.
pub fn clear(host: &web_sys::Element) {
    let _ = host.remove_attribute(ACTIVE_ATTR);
    let Ok(nodes) = host.query_selector_all(&format!("[{BOOT_ATTR}]")) else {
        return;
    };
    for index in 0..nodes.length() {
        let Some(node) = nodes.item(index) else {
            continue;
        };
        if let Some(parent) = node.parent_node() {
            let _ = parent.remove_child(&node);
        }
    }
}
