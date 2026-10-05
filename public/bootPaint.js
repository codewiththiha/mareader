// The shell page's first paint, in the reader's own paper.
//
// The page shows #shell-boot while the shell wasm downloads and starts, and
// the runtime host's loading cover after it. Both are painted before any
// Rust runs, so on their own they can only wear the stylesheet's default
// (light) paper: a dark or tinted theme opened with a white flash. The
// shell's theme effect (src/effects/app/theme.rs) stores the resolved paper
// and colour scheme under BOOT_PAINT_KEY every time the look changes; this
// script reads it back synchronously in <head>, before the body exists, so
// the very first frame is already the right colour.
//
// Only a custom property and color-scheme are written, never `background`:
// the theme painter rebuilds <html>'s inline style but carries every
// declaration it does not own verbatim, so an inline background would
// outlive the next theme switch. styles/boot.css reads --boot-paper only
// until the shell's first paint sets data-base on <html>.
//
// A classic external script, not inline: the packaged app's CSP is
// `script-src 'self' 'wasm-unsafe-eval'` (src-tauri/tauri.conf.json).
// Copied to the dist root by index.html and required by
// tools/check-runtime-artifacts.mjs. Best-effort: any failure (storage
// blocked, first run) leaves the stylesheet's default paper.
(function () {
  var BOOT_PAINT_KEY = "mareader.boot-paint.v1";
  try {
    var value = window.localStorage.getItem(BOOT_PAINT_KEY);
    if (!value) return;
    var split = value.lastIndexOf("|");
    var paper = split < 0 ? value : value.slice(0, split);
    var scheme = split < 0 ? "" : value.slice(split + 1);
    var style = document.documentElement.style;
    if (paper) style.setProperty("--boot-paper", paper);
    if (scheme === "dark" || scheme === "light") style.setProperty("color-scheme", scheme);
  } catch (_) {
    // No storage, no remembered paper: the default one is correct enough.
  }
})();
