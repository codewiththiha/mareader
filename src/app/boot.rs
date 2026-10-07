//! The runtime host's boot states: the loading cover and the error
//! card.

use serde_json::json;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

/// Which runtime a boot state or failure is about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuntimeName {
    Library,
    Reader,
}

impl RuntimeName {
    /// The runtime's name in the error card and diagnostics.
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
    /// The outgoing session is being torn down.
    Dispose,
    /// The runtime's session never answered its offer.
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

    /// The machine-readable form the native boot report and `to_json` carry.
    pub const fn slug(self) -> &'static str {
        match self {
            BootStage::Dispose => "dispose",
            BootStage::ModuleLoad => "module-load",
            BootStage::Init => "init",
            BootStage::Start => "start",
        }
    }

    /// What the boot was doing, in the user's terms.
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

/// A runtime that did not start.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BootError {
    pub runtime: RuntimeName,
    pub stage: BootStage,
    pub message: String,
}

/// The longest failure text the UI shows.
const MAX_DETAIL: usize = 240;

impl BootError {
    pub fn new(runtime: RuntimeName, stage: BootStage, message: impl AsRef<str>) -> Self {
        // One line, bounded: the first line holds the useful part.
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

    /// What the console gets, in the words the UI uses.
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

/// The shell's boot phase, published as a signal.
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

// --- the DOM: everything the host paints carries `data-mareader-boot` ---

const BOOT_ATTR: &str = "data-mareader-boot";
/// Set on the host itself once a runtime is mounted (the active state).
const ACTIVE_ATTR: &str = "data-mareader-active";
/// Which boot node is the shell's loading state, as opposed to the error state.
const LOADING: &str = "load";

fn document() -> Option<web_sys::Document> {
    web_sys::window().and_then(|w| w.document())
}

fn element(tag: &str) -> Option<web_sys::Element> {
    document().and_then(|d| d.create_element(tag).ok())
}

/// The app's own loading mark, `app_ui`'s Loader shape.
fn loader_mark(parent: &web_sys::Element) {
    let Some(mark) = element("div") else {
        return;
    };
    let _ = mark.set_attribute("class", "loader runtime-boot__loader");
    let _ = mark.set_attribute("aria-hidden", "true");
    for dot in ["a", "b", "c"] {
        if let Some(node) = element("span") {
            let _ = node.set_attribute("class", &format!("loader-dot loader-dot-{dot}"));
            let _ = mark.append_child(&node);
        }
    }
    let _ = parent.append_child(&mark);
}

fn text_node(parent: &web_sys::Element, tag: &str, class: &str, text: &str) {
    let Some(node) = element(tag) else {
        return;
    };
    let _ = node.set_attribute("class", class);
    node.set_text_content(Some(text));
    let _ = parent.append_child(&node);
}

/// The loading card itself.
fn loading_card(runtime: RuntimeName) -> Option<web_sys::Element> {
    let card = element("div")?;
    let _ = card.set_attribute("class", "runtime-boot");
    let _ = card.set_attribute(BOOT_ATTR, LOADING);
    let _ = card.set_attribute("role", "status");
    let _ = card.set_attribute("aria-live", "polite");
    loader_mark(&card);
    text_node(&card, "p", "runtime-boot__title", "Loading MAReader…");
    let hint = format!("Starting the {} runtime", runtime.label());
    text_node(&card, "p", "runtime-boot__hint", &hint);
    Some(card)
}

/// The shell's loading state, painted before any await.
pub fn paint_loading(host: &web_sys::Element, runtime: RuntimeName) {
    let _ = host.remove_attribute(ACTIVE_ATTR);
    clear_loading(host);
    cover(host, runtime);
}

/// Put the loading state back if the host has none.
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

/// Remove the shell's LOADING state only.
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

/// Remove the page's own placeholder (`#shell-boot`).
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

/// The error state; the button reloads the window.
pub fn paint_error(host: &web_sys::Element, error: &BootError) {
    clear(host);
    let Some(card) = element("div") else {
        return;
    };
    let _ = card.set_attribute("class", "runtime-boot runtime-boot--error");
    let _ = card.set_attribute(BOOT_ATTR, "error");
    // The browser suites assert on this, and a support report can quote it.
    let _ = card.set_attribute("data-mareader-runtime", error.runtime.artifact());
    let _ = card.set_attribute("role", "alert");
    text_node(&card, "p", "runtime-boot__title", &error.headline());
    text_node(&card, "p", "runtime-boot__hint", &error.context());
    text_node(&card, "p", "runtime-boot__detail", &error.message);
    let reload = js_sys::Function::new_no_args("window.location.reload()");
    if let Some(button) = retry_button(&reload, "Reload MAReader") {
        let _ = card.append_child(&button);
    }
    let _ = host.append_child(&card);
    // The console keeps the detail: one line, with the failure.
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

/// Mark the host as holding a live runtime.
pub fn set_active(host: &web_sys::Element, runtime: RuntimeName) {
    let _ = host.set_attribute(ACTIVE_ATTR, runtime.artifact());
}

/// Remove the shell's boot markup only.
pub fn clear_boot(host: &web_sys::Element) {
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

/// Remove the shell's boot markup and the active stamp.
pub fn clear(host: &web_sys::Element) {
    let _ = host.remove_attribute(ACTIVE_ATTR);
    clear_boot(host);
}
