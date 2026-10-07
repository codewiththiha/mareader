// Tauri IPC for the runtime frames.
//
// CVE-2024-35222 (patched in Tauri >= 2.0.0-beta.20) removed the Tauri
// initialization script from SUB-FRAMES on every platform — until then macOS
// had injected it into iframes. MAReader runs both runtimes in iframes, so
// `window.__TAURI__` does not exist where the reader reads files and the
// library reveals them: every local-file open degraded to
// `fetch("/Users/…")`,404'd, and the reader sat on "No document" with an
// empty viewer while CI (a plain browser, http-served samples) stayed green.
//
// The shell page — this frame's PARENT — is the main frame and does carry the
// API and its invoke key. When, and only when, an ancestor is same-origin and
// exposes `__TAURI__`, publish a facade on this frame's `window` whose every
// namespace and method runs there. Results cross as same-origin references
// (parent and frame share origin and process), so promises, listener handles
// and API objects behave as if the call happened here — which, for
// authorization, it did: the main window holds the `__TAURI_INVOKE_KEY__` the
// backend accepts.
//
// On Windows, wry's initialization-script API documents "**Windows:** scripts
// are always added to subframes", so a frame there is injected with its OWN
// `__TAURI__` and never needs the facade — which is why file IO, dialogs and
// reveals have always worked on Windows. Two halves of that injected API are
// still wrong from inside a frame, and both read as a dead title bar:
//
//   * `event.listen` registers in the LISTENING frame's own registry, while the
//     backend delivers an event by scripting the MAIN frame. A frame listening
//     on its own API therefore never hears `tauri://resize` (the caption's
//     maximized probe), `tauri://focus`, the shelf's cover-progress channel or
//     `ai-stream-chunk`. So even a frame that has an API borrows the host's
//     `event` namespace; every other namespace stays local, where it works.
//   * `data-tauri-drag-region` is not a platform feature either: it is a
//     mousedown listener in the same initialization script. It IS present on
//     Windows, but it recognises a region only when the pressed element
//     carries the attribute itself — and MAReader's bars are CONTAINERS whose
//     every pixel belongs to a child, so nothing was grabbable. This script
//     installs the same listener on every platform, and teaches it the `deep`
//     spelling that claims a container's whole subtree. Where Tauri's own copy
//     IS present — the shell page, and any frame Windows injects — an
//     initialization script runs before this file and stops immediate
//     propagation the moment it acts, so one press is never handled twice: the
//     injected script takes a press on an element that carries the attribute,
//     this one takes every press its own rule declined.
//
// Guards: the drag listener needs only "this document is a frame"; the facade
// additionally needs a webview that has Proxy and an ancestor with the API, and
// says so in the console when there is none. Where no facade is published,
// `tauri_bridge::has_tauri()` stays honest about what is really available.
(function () {
  "use strict";

  // This document is a frame when its parent is not itself — and never when
  // reading the parent throws, which is what a cross-origin ancestor does.
  var inFrame;
  try {
    inFrame = window.parent !== window;
  } catch (_) {
    inFrame = false;
  }
  if (!inFrame) return;

  // The API a call from this frame should go to: this frame's own where it has
  // one that can invoke (Windows), otherwise the nearest ancestor that does.
  // The top frame is tried first because it is the main frame — the only
  // document guaranteed both to hold a real API and to be where the backend
  // delivers events.
  function hostRoot() {
    var root = null;
    try {
      root = window.top.__TAURI__;
    } catch (_) {
      root = null;
    }
    if (root && typeof root === "object") return root;
    try {
      root = window.parent.__TAURI__;
    } catch (_) {
      return null; // cross-origin ancestor: not ours to relay through
    }
    return root && typeof root === "object" ? root : null;
  }

  function facade(target) {
    return new Proxy(target, {
      get: function (t, prop) {
        if (typeof prop === "symbol") return Reflect.get(t, prop);
        var value = Reflect.get(t, prop);
        if (value === null || (typeof value !== "object" && typeof value !== "function")) {
          return value;
        }
        // Proxy invariant: for a non-configurable, non-writable own data
        // property the 'get' result MUST be the target's exact value —
        // returning a wrapped object here throws a TypeError on access
        // (which is exactly how the document open died: the engine's
        // convertFileSrc/invoke sit behind such a property). Pinned
        // properties are therefore returned RAW; anything else may carry
        // its facade.
        var desc = Object.getOwnPropertyDescriptor(t, prop);
        if (desc && desc.configurable === false && desc.writable === false) {
          return value;
        }
        if (typeof value === "function") {
          // A method: run it on the parent's own object so `this` is right.
          // A namespace: recurse. (Methods are functions, namespaces are
          // plain objects — the two cases do not overlap.)
          return function () {
            return Reflect.apply(value, t, arguments);
          };
        }
        return facade(value);
      },
    });
  }

  // ------------------------------------------------------------- window drag
  // The list Tauri's own drag script uses, so the frames and the shell page
  // agree about where the window is grabbable. A clickable element that does
  // not carry the attribute itself claims the press for its own click.
  var CLICKABLE_TAGS = { A: 1, BUTTON: 1, INPUT: 1, SELECT: 1, TEXTAREA: 1, LABEL: 1, SUMMARY: 1 };
  var INTERACTIVE_ROLES = {
    button: 1, link: 1, menuitem: 1, tab: 1, checkbox: 1, radio: 1, switch: 1, option: 1,
  };

  function isClickable(el) {
    if (CLICKABLE_TAGS[el.tagName]) return true;
    if (el.hasAttribute("contenteditable") && el.getAttribute("contenteditable") !== "false") {
      return true;
    }
    // tabindex="-1" is not interactivity: it only makes the element script-focusable.
    if (el.hasAttribute("tabindex") && el.getAttribute("tabindex") !== "-1") return true;
    return INTERACTIVE_ROLES[el.getAttribute("role")] === 1;
  }

  // ported from tauri/src/window/scripts/drag.js at tauri-v2.11.5 — keep the
  // two in step, or the shell page's bar and the frames' bars disagree.
  function isDragRegion(path) {
    for (var i = 0; i < path.length; i++) {
      var el = path[i];
      if (!(el instanceof HTMLElement)) continue;
      var attr = el.getAttribute("data-tauri-drag-region");
      if (isClickable(el) && attr === null) return false;
      if (attr === null) continue;
      if (attr === "false") return false;
      if (attr === "deep") return true;
      if (attr === "" || attr === "true") return el === path[0];
    }
    return false;
  }

  // The sender for one press, or null while no Tauri API is reachable. Null has
  // to be answered BEFORE the press is claimed: a drag that cannot happen must
  // not also eat the page's own text selection, caret or default click — which
  // is what a plain browser (and any frame mid-startup) would otherwise get.
  function dragSender() {
    var local = window.__TAURI__;
    var root = local && local.core && typeof local.core.invoke === "function" ? local : hostRoot();
    var invoke = root && root.core && typeof root.core.invoke === "function" ? root.core.invoke : null;
    if (!invoke) return null;
    return function press(cmd) {
      try {
        var p = invoke(cmd);
        if (p && typeof p.catch === "function") {
          p.catch(function (err) {
            console.warn("[mareader] tauri-relay: " + cmd + " failed", err);
          });
        }
      } catch (err) {
        console.warn("[mareader] tauri-relay: " + cmd + " threw", err);
      }
    };
  }

  // macOS maximizes on mouseup, and cancels when the pointer has moved since
  // the double-click's press (the same grace the native title bar gives). The
  // frame cannot read the OS from Tauri's injection template, so this is the
  // platform's own answer to "am I on macOS".
  var defersMaximize = /^(Mac|iPhone|iPod|iPad)/.test(
    navigator.platform || navigator.userAgent,
  );
  var pressX = 0;
  var pressY = 0;

  document.addEventListener("mousedown", function (e) {
    if (e.button !== 0 || (e.detail !== 1 && e.detail !== 2)) return;
    if (!isDragRegion(e.composedPath())) return;
    var send = dragSender();
    if (!send) return;
    if (defersMaximize && e.detail === 2) {
      pressX = e.clientX;
      pressY = e.clientY;
      return;
    }
    e.preventDefault(); // no text cursor while the window moves
    e.stopImmediatePropagation();
    send(e.detail === 2 ? "plugin:window|internal_toggle_maximize" : "plugin:window|start_dragging");
  });

  if (defersMaximize) {
    document.addEventListener("mouseup", function (e) {
      if (
        e.button === 0 &&
        e.detail === 2 &&
        e.clientX === pressX &&
        e.clientY === pressY &&
        isDragRegion(e.composedPath())
      ) {
        var send = dragSender();
        if (send) send("plugin:window|internal_toggle_maximize");
      }
    });
  }

  // ------------------------------------------------------------- the facade
  if (typeof Proxy !== "function") return; // old webview: no facade, no harm
  var root = hostRoot();
  if (!root) {
    console.warn(
      "[mareader] tauri-relay: this frame has no Tauri API and its parent " +
        "does not expose one — file IO in this frame cannot work",
    );
    return;
  }
  var local = window.__TAURI__;
  if (local === root) return; // already the host's own object
  try {
    if (!local) {
      window.__TAURI__ = facade(root);
      console.info("[mareader] tauri-relay: Tauri API republished in this frame");
    } else {
      // Windows: this frame was injected with its own working API, so only
      // `event` moves — the one namespace a sub-frame cannot use on its own.
      // Refuse to wrap it if the property cannot be re-pointed: a proxy that
      // violates the invariant would throw on every access, and a frame with
      // dead events is the status quo, not a regression.
      var desc = Object.getOwnPropertyDescriptor(local, "event");
      if (!desc || (!desc.configurable && !desc.writable)) return;
      var events = facade(root).event;
      var merged = new Proxy(local, {
        get: function (t, prop) {
          return prop === "event" ? events : Reflect.get(t, prop, t);
        },
      });
      window.__TAURI__ = merged;
      if (window.__TAURI__ !== merged) return;
      console.info(
        "[mareader] tauri-relay: this frame keeps its own Tauri API and " +
          "registers its events on the host frame",
      );
    }
  } catch (err) {
    console.warn("[mareader] tauri-relay: publishing the facade failed", err);
  }
})();

