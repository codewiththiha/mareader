//! The runtime host's boot states: what the user sees in `#runtime-host` while
//! a runtime is loading, and what they see when one cannot start.
//!
//! Two invariants live here (§5, §6, §11):
//!
//! * the host is NEVER empty — it holds a loading state, an active runtime, or
//!   an error state, and the manager moves from one to the next without an
//!   await in between (an empty host is a blank window);
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
        format!("MAReader could not start the {} runtime", self.runtime.label())
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

    /// Is the shell still using the page's own placeholder? Only here.
    pub fn is_page_placeholder(&self) -> bool {
        matches!(self, BootPhase::Booting)
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

/// The shell's loading state, painted into the host before any await.
pub fn paint_loading(host: &web_sys::Element, runtime: RuntimeName) {
    clear(host);
    let Some(card) = element("div") else {
        return;
    };
    let _ = card.set_attribute("class", "runtime-boot");
    let _ = card.set_attribute(BOOT_ATTR, "load");
    let _ = card.set_attribute("role", "status");
    let _ = card.set_attribute("aria-live", "polite");
    text_node(&card, "p", "runtime-boot__title", "Loading MAReader…");
    let hint = format!("Starting the {} runtime", runtime.label());
    text_node(&card, "p", "runtime-boot__hint", &hint);
    let _ = host.append_child(&card);
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

/// Mark the host as holding a live runtime. Called after the runtime's start
/// export returned — the runtime has mounted its own DOM by then.
pub fn mark_active(host: &web_sys::Element, runtime: RuntimeName) {
    clear(host);
    let _ = host.set_attribute(ACTIVE_ATTR, runtime.artifact());
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
