// The shell page's first paint, in the reader's own paper.
(function () {
  var BOOT_PAINT_KEY = "mareader.boot-paint.v1";

  // The same colour as red/green/blue, or null if unsayable.
  function toRgb(color) {
    try {
      var canvas = document.createElement("canvas");
      canvas.width = 1;
      canvas.height = 1;
      var ctx = canvas.getContext("2d", { willReadFrequently: true });
      if (!ctx) return null;
      // Magenta first: a rejected fill leaves the previous value.
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

  // Hand the paper to the native window, best-effort.
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
