// Tauri IPC for the runtime frames: a same-origin facade, and the
// drag-region listener.
(function () {
  "use strict";

  // This document is a frame when its parent is not itself.
  var inFrame;
  try {
    inFrame = window.parent !== window;
  } catch (_) {
    inFrame = false;
  }
  if (!inFrame) return;

  // The API a call from this frame goes to: its own, else an ancestor's.
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
        // Proxy invariant: pinned properties must be returned raw.
        var desc = Object.getOwnPropertyDescriptor(t, prop);
        if (desc && desc.configurable === false && desc.writable === false) {
          return value;
        }
        if (typeof value === "function") {
          // A method runs on the parent's object; a namespace recurses.
          return function () {
            return Reflect.apply(value, t, arguments);
          };
        }
        return facade(value);
      },
    });
  }

  // --- window drag: the list Tauri's own script uses, so bars agree ---
  var CLICKABLE_TAGS = { A: 1, BUTTON: 1, INPUT: 1, SELECT: 1, TEXTAREA: 1, LABEL: 1, SUMMARY: 1 };
  var INTERACTIVE_ROLES = {
    button: 1, link: 1, menuitem: 1, tab: 1, checkbox: 1, radio: 1, switch: 1, option: 1,
  };

  function isClickable(el) {
    if (CLICKABLE_TAGS[el.tagName]) return true;
    if (el.hasAttribute("contenteditable") && el.getAttribute("contenteditable") !== "false") {
      return true;
    }
    // tabindex="-1" is not interactivity.
    if (el.hasAttribute("tabindex") && el.getAttribute("tabindex") !== "-1") return true;
    return INTERACTIVE_ROLES[el.getAttribute("role")] === 1;
  }

  // Ported from tauri-v2.11.5's drag.js; keep the two in step.
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

  // The sender for one press, or null when no API is reachable.
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

  // macOS maximizes on mouseup, and cancels when the pointer moved.
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
      // Windows: only `event` moves; a frame's own API cannot hear events.
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
