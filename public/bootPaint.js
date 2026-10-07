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
// Only a custom property and color-scheme are written to the page, never
// `background`: the theme painter rebuilds <html>'s inline style but carries
// every declaration it does not own verbatim, so an inline background would
// outlive the next theme switch. styles/boot.css reads --boot-paper only until
// the shell's first paint sets data-base on <html>.
//
// The paper is also handed to the native window, which no stylesheet can reach
// (see paintWindow below): that is the half of a launch the page cannot paint,
// and on Windows it is white by default.
//
// A classic external script, not inline: the packaged app's CSP is
// `script-src 'self' 'wasm-unsafe-eval'` (src-tauri/tauri.conf.json).
// Copied to the dist root by index.html and required by
// tools/check-runtime-artifacts.mjs. Best-effort: any failure (storage
// blocked, first run) leaves the stylesheet's default paper.
(function () {
  var BOOT_PAINT_KEY = "mareader.boot-paint.v1";

  // The same colour as red / green / blue, or null if this webview cannot say.
  // The remembered paper is whatever CSS the theme resolved to — an oklch() with
  // a tint on it — and the native window wants numbers, so the resolution is
  // asked of the one engine that is guaranteed to agree with the page: the
  // canvas, which parses any CSS colour and hands back the pixel it made.
  function toRgb(color) {
    try {
      var canvas = document.createElement("canvas");
      canvas.width = 1;
      canvas.height = 1;
      var ctx = canvas.getContext("2d", { willReadFrequently: true });
      if (!ctx) return null;
      // Magenta first, because a rejected `fillStyle` assignment leaves the
      // previous value in place: seeing it back means this canvas cannot parse
      // the colour (an engine older than the theme's oklch), and "no answer"
      // is worth more than a confident black window.
      ctx.fillStyle = "#ff00ff";
      ctx.fillRect(0, 0, 1, 1);
      ctx.fillStyle = color;
      if (ctx.fillStyle === "#ff00ff") return null;
      ctx.fillRect(0, 0, 1, 1);
      var pixel = ctx.getImageData(0, 0, 1, 1).data;
      if (pixel[3] === 255 && !(pixel[0] === 255 && pixel[1] === 0 && pixel[2] === 255)) {
        return [pixel[0], pixel[1], pixel[2]];
      }
      return null;
    } catch (_) {
      return null;
    }
  }

  // Hand the paper to the window itself. The page can be painted from its own
  // first frame; the NATIVE window behind it cannot, and WebView2's default is
  // white — which is what a Windows launch showed before the page existed, and
  // what flashes at the edges while the window is resized. A call into
  // `__TAURI__` is asynchronous, but this runs in <head>, so the answer lands
  // at about the page's own first paint: the launch stops having a white
  // moment. Best-effort twice over — no Tauri (a browser tab), no window to
  // colour, and no permission, which the backend reports as a rejected invoke.
  function paintWindow(color) {
    var tauri = window.__TAURI__;
    if (!tauri || !tauri.window || typeof tauri.window.getCurrentWindow !== "function") return;
    var rgb = toRgb(color);
    if (!rgb) return;
    try {
      var p = tauri.window.getCurrentWindow().setBackgroundColor(rgb);
      if (p && typeof p.catch === "function") p.catch(function () {});
    } catch (_) {
      /* no window to paint */
    }
  }

  try {
    var value = window.localStorage.getItem(BOOT_PAINT_KEY);
    if (!value) return;
    var split = value.lastIndexOf("|");
    var paper = split < 0 ? value : value.slice(0, split);
    var scheme = split < 0 ? "" : value.slice(split + 1);
    var style = document.documentElement.style;
    if (paper) style.setProperty("--boot-paper", paper);
    if (scheme === "dark" || scheme === "light") style.setProperty("color-scheme", scheme);
    if (paper) paintWindow(paper);
  } catch (_) {
    // No storage, no remembered paper: the default one is correct enough.
  }
})();
