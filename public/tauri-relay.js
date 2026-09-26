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
// API and its invoke key. When, and only when, the parent is same-origin and
// exposes `__TAURI__`, publish a facade on this frame's `window` whose every
// namespace and method runs on the parent. Results cross as same-origin
// references (parent and frame share origin and process), so promises,
// listener handles and API objects behave as if the call happened here —
// which, for authorization, it did: the parent window holds the
// `__TAURI_INVOKE_KEY__` the backend accepts.
//
// Guards, in order: the API already exists here (the shell itself, or a
// same-origin frame that still gets injected), a top-level document (plain
// browser dev, standalone library.html), a cross-origin parent, a parent
// without the API (`trunk serve` in a browser), and a webview without Proxy.
// In every one of those this script does nothing, and
// `tauri_bridge::has_tauri()` stays honest about what is really available.
(function () {
  "use strict";
  if (window.__TAURI__) return;
  if (typeof Proxy !== "function") return;
  if (window.parent === window) return;
  var root;
  try {
    root = window.parent.__TAURI__;
  } catch (_) {
    return; // cross-origin parent: not ours to relay through
  }
  if (!root || typeof root !== "object") {
    // One level up is the only same-origin candidate worth trying (a nested
    // shell), then say NOTHING was found — the line this prints in devtools
    // is the difference between "the relay silently skipped" and a bug one
    // look can name.
    try {
      root = window.top.__TAURI__;
    } catch (_) {
      root = null;
    }
    if (!root || typeof root !== "object") {
      console.warn(
        "[mareader] tauri-relay: this frame has no Tauri API and its parent " +
          "does not expose one — file IO in this frame cannot work",
      );
      return;
    }
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

  window.__TAURI__ = facade(root);
  console.info("[mareader] tauri-relay: Tauri API republished in this frame");
})();
