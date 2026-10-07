// The frame-side window chrome check.
//
// The bar, the caption and the file dialogs all live in a ROUTE FRAME, and a
// frame's Tauri surface differs by platform: macOS and Linux get no
// initialization script in sub-frames (CVE-2024-35222), so
// `public/tauri-relay.js` republishes the host frame's API there; Windows is
// documented the other way round ("scripts are always added to subframes"), so
// a frame holds its own real API there — which is also how the frame loses both
// its drag region (Tauri's own script acts only on an element that carries the
// attribute itself, and the bar is a CONTAINER) and its event listeners
// (`emit` is delivered by scripting the MAIN frame, so a frame-local registry is
// never read). Both failures shipped as "the window cannot be moved" and "the
// caption never notices it is maximized".
//
// So this runs the REAL script — in a `vm`, against a two-class DOM — over the
// three platform shapes, and asserts what a press and a listener registration
// are allowed to do. A browser suite cannot cover this: it has no Tauri at all,
// and a webview smoke has no window manager to honour a drag.
//
// JavaScript, not TypeScript: like tools/check-tauri-contract.mjs it must run
// before node_modules exists.

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const RELAY = path.join(root, "public/tauri-relay.js");

const problems = [];
const fail = (message) => problems.push(message);
const ok = (label, condition, detail) => {
  if (!condition) {
    fail(`${label}${detail === undefined ? "" : ` — saw ${JSON.stringify(detail)}`}`);
  }
};

// ---------------------------------------------------------------- the DOM
// Only what the script touches: attributes, a composed path, an event listener
// table. `HTMLElement` is the class the walk accepts, so anything that is not
// one (the document, the window) must be skipped rather than mistaken for a
// region.
class Element {
  constructor(tag, attrs = {}, parent = null) {
    this.tagName = tag.toUpperCase();
    this.attrs = attrs;
    this.parentElement = parent;
  }

  getAttribute(name) {
    return name in this.attrs ? this.attrs[name] : null;
  }

  hasAttribute(name) {
    return name in this.attrs;
  }

  composedPath() {
    const path = [];
    for (let el = this; el; el = el.parentElement) path.push(el);
    path.push({ nodeType: 9 }, { nodeType: 0 });
    return path;
  }
}

const DRAG = "data-tauri-drag-region";
const START = "plugin:window|start_dragging";
const TOGGLE = "plugin:window|internal_toggle_maximize";

/** The bar's real shape: a `deep` band over a `deep` row, a leading cluster
 *  that stays self-only, a display span that carries the attribute itself, a
 *  search pill that opts its whole subtree out, and a button. */
function bar() {
  const band = new Element("div", { [DRAG]: "deep" });
  const row = new Element("div", { [DRAG]: "deep" }, band);
  const leading = new Element("div", { [DRAG]: "true" }, row);
  const crumb = new Element("span", {}, leading);
  const pin = new Element("button", {}, leading);
  const anchor = new Element("div", {}, row);
  const pill = new Element("div", { [DRAG]: "false" }, anchor);
  const field = new Element("input", {}, pill);
  const title = new Element("span", { [DRAG]: "true" }, row);
  return { band, row, leading, crumb, pin, anchor, pill, field, title };
}

/** One Tauri API object: every invoke and every `listen` lands in a log. */
function tauriApi(log) {
  return {
    metadata: { currentWindow: { label: "main" } },
    core: {
      invoke(cmd) {
        log.invoke.push(cmd);
        return Promise.resolve(undefined);
      },
    },
    event: {
      listen(name) {
        log.listen.push(name);
        return Promise.resolve(() => {});
      },
    },
    window: {
      getCurrentWindow() {
        return { isMaximized: () => Promise.resolve(false) };
      },
    },
  };
}

/** Run the shipped relay as one document of a given shape. */
function frame({ platform, host = null, own = null, injected = false }) {
  const log = { invoke: [], listen: [] };
  const hostLog = { invoke: [], listen: [] };
  const hostRoot = host === "stub" ? tauriApi(hostLog) : host;
  const document = {
    listeners: {},
    addEventListener(type, handler) {
      (this.listeners[type] ||= []).push(handler);
    },
  };
  const parent = { __TAURI__: hostRoot };
  const self = { parent, top: parent };
  // A real frame either has the global or does not have it at all — an absent
  // `__TAURI__` is `undefined`, and the relay reads that difference.
  if (own === "stub") self.__TAURI__ = tauriApi(log);
  else if (own) self.__TAURI__ = own;
  parent.parent = parent;
  parent.top = parent;
  const sandbox = {
    window: self,
    document,
    HTMLElement: Element,
    navigator: { platform },
    console: { log() {}, info() {}, warn() {}, error() {} },
  };
  if (injected) {
    // Tauri's `drag.js`, reduced to the rule that matters here: it acts on an
    // element that carries the attribute itself, and consumes the event.
    document.addEventListener("mousedown", (event) => {
      if (event.detail !== 1 && event.detail !== 2) return;
      if (event.target.getAttribute?.(DRAG) === null) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      hostLog.invoke.push(event.detail === 2 ? TOGGLE : START);
      event.consumed = "injected";
    });
  }
  vm.createContext(sandbox);
  vm.runInContext(fs.readFileSync(RELAY, "utf8"), sandbox);

  const dispatch = (type, el, { detail = 1, button = 0, x = 7, y = 9 } = {}) => {
    const event = {
      type, button, detail, target: el, clientX: x, clientY: y,
      composedPath: () => el.composedPath(),
      get prevented() { return event._prevented === true; },
      preventDefault() { event._prevented = true; },
      stopImmediatePropagation() { event._stopped = true; },
    };
    for (const handler of document.listeners[type] ?? []) {
      handler(event);
      if (event._stopped) break;
    }
    return {
      prevented: event.prevented,
      by: event.consumed ?? "relay",
      host: hostLog.invoke.splice(0),
      local: log.invoke.splice(0),
    };
  };

  return {
    self,
    log,
    hostLog,
    press: (el, options) => dispatch("mousedown", el, options),
    release: (el, options) => dispatch("mouseup", el, options),
    listeners: () => [...hostLog.listen, ...log.listen],
    hostListenerCount: () => hostLog.listen.length,
  };
}

// ------------------------------------------------------- macOS and Linux: no
// API in the frame, so the facade is the frame's only surface.
{
  const f = frame({ platform: "Linux x86_64", host: "stub" });
  const b = bar();
  ok("the frame gets a facade, not a copy of the host object", f.self.__TAURI__ && f.self.__TAURI__ !== f.self.parent.__TAURI__, f.self.__TAURI__ === undefined);
  const through = f.press(b.anchor);
  ok("a press on a child of the bar's deep region drags the window", through.prevented && through.host.join() === START, through);
  ok("a press on the bar itself drags the window", f.press(b.row).host.join() === START);
  ok("a press on the band drags the window", f.press(b.band).host.join() === START);
  ok("a display leaf that carries the attribute drags the window", f.press(b.title).host.join() === START);
  ok("a bar button keeps its own click", f.press(b.pin).host.length === 0);
  ok("the search field's pill opts out, caret included", f.press(b.field).prevented === false && f.press(b.pill).prevented === false);
  ok("a cluster whose region is self-only claims only itself", f.press(b.crumb).host.length === 0);  const outside = new Element("p", {}, new Element("div", {}));
  ok("a press outside any region is left to the page", f.press(outside).prevented === false && f.press(outside).host.length === 0);
  ok("a double-click toggles maximization", f.press(b.anchor, { detail: 2 }).host.join() === TOGGLE);
  ok("the release does nothing off macOS", f.release(b.anchor, { detail: 2 }).host.length === 0);
  ok("the right button never drags", f.press(b.anchor, { button: 2 }).host.length === 0);
  // The shell page's OWN copy of the listener is registered before this file
  // runs (an init script precedes page scripts). When it acts it stops immediate
  // propagation, so one press must never reach the backend twice; when it
  // declines — a child of a self-only region, exactly the case that used to make
  // the bar dead — this one has to act.
  const both = frame({ platform: "Linux x86_64", host: "stub", injected: true });
  const bb = bar();
  const onRow = both.press(bb.row);
  ok("one press, one command, whichever script acted", onRow.host.length === 1 && onRow.by === "injected", onRow);
  const onAnchor = both.press(bb.anchor);
  ok("the relay covers exactly what the injected script declined", onAnchor.host.join() === START, onAnchor);
}

// ------------------------------------------------- macOS: maximize on release
{
  const f = frame({ platform: "MacIntel", host: "stub" });
  const b = bar();
  ok("macOS defers the double-click to the release", f.press(b.anchor, { detail: 2 }).host.length === 0);
  ok("an unmoved release maximizes", f.release(b.anchor, { detail: 2 }).host.join() === TOGGLE);
  const moved = { detail: 2, x: 400, y: 300 };
  f.press(b.anchor, { detail: 2 });
  ok("a drag away cancels it", f.release(b.anchor, moved).host.length === 0);
}

// ------------------------- Windows: the frame HAS its own injected API, and it
// must still be able to move the window and hear the backend.
{
  const f = frame({ platform: "Win32", host: "stub", own: "stub" });
  const b = bar();
  ok("the frame keeps a wrapper instead of its bare API", f.self.__TAURI__ !== undefined);
  ok("a deep child drags even though Tauri's own script declined", f.press(b.anchor).local.join() === START, f.press(b.anchor));
  ok("the frame's own API still answers its commands", (f.self.__TAURI__.core.invoke("x"), f.log.invoke.join() === "x"));
  await f.self.__TAURI__.event.listen("tauri://resize", () => {});
  ok("a frame's listener is registered where the backend can reach it", f.hostListenerCount() === 1, f.listeners());
  ok("the frame's own unreachable registry is left alone", f.log.listen.length === 0);
}

// ---------------------------------------------------- a plain browser: nothing
{
  const f = frame({ platform: "Linux x86_64" });
  const b = bar();
  const press = f.press(b.anchor);
  ok("no API: the press is not claimed, so the page still works", press.prevented === false && press.host.length === 0 && press.local.length === 0, press);
  ok("no API: no listener pretends the window can move", f.self.__TAURI__ === undefined);
}

// --------------------------------------- the pairing the behaviour depends on
// The relay implements `deep`; the chrome has to ASK for it, or the whole subtree
// rule is dead code. Both sides are pinned so neither can drift alone.
{
  const chrome = fs.readFileSync(path.join(root, "crates/app-chrome/src/titlebar/root.rs"), "utf8");
  const deep = chrome.match(/data-tauri-drag-region="deep"/g) ?? [];
  ok("the title bar's band and row ask for a deep region", deep.length === 2, deep.length);
  const sidebar = fs.readFileSync(path.join(root, "crates/reader-runtime/src/components/shell/sidebar/header.rs"), "utf8");
  ok("the sidebar header asks for a deep region", sidebar.includes('data-tauri-drag-region="deep"'));
  const search = fs.readFileSync(path.join(root, "crates/library-runtime/src/features/library/titlebar_search.rs"), "utf8");
  ok("the search pill opts its subtree out", search.includes('data-tauri-drag-region="false"'));
}

if (problems.length > 0) {
  console.error(`tauri relay: ${problems.length} violation(s)\n`);
  for (const problem of problems) console.error(`  ${problem}`);
  process.exit(1);
}
console.log("tauri relay: drag regions, the facade and the host-frame event registry behave on all three platforms");

