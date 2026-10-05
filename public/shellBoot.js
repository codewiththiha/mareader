// The shell page's boot screen: what the wait says, and when it says enough.
//
// `#shell-boot` (index.html) is the placeholder the user sees before the shell
// wasm exists. The shell removes it as soon as the runtime host paints its
// first state. This script owns the other half of that contract: the copy gets
// more specific as the wait grows, so a slow launch explains itself instead of
// showing a mark that never stops, and a start that never lands says so.
//
// The stages are facts about this app's startup, in the order they happen: the
// page paints, the shell's wasm is fetched and compiled, its first mount takes
// the placeholder away, and the runtime host carries the wait from there (its own
// cover is painted by src/app/boot.rs). There is no index, cache or database
// step to wait for — the library is a directory listing plus localStorage — so
// an unhurried blank window on a machine whose webview is slow to paint was
// easily read as "it's setting something up". It is not; it is still loading.
//
// A classic external script, not inline: the packaged app's CSP is
// `script-src 'self' 'wasm-unsafe-eval'` (src-tauri/tauri.conf.json), which
// blocks inline scripts. Copied to the dist root by index.html and required by
// tools/check-runtime-artifacts.mjs.
(function () {
  // A document opened straight from the filesystem can never start: wasm
  // modules and the root-absolute URLs this page is built from need a real
  // origin. Say so at once instead of spinning through the 20 s deadline.
  if (window.location.protocol === "file:") {
    var early = document.getElementById("shell-boot");
    if (early) {
      early.setAttribute("data-shell-boot", "file-protocol");
      var earlyHint = early.querySelector(".shell-boot__hint");
      if (earlyHint) {
        earlyHint.textContent =
          "This page must be served over HTTP — run npm run dev:frontend; a file:// window cannot start WebAssembly.";
      }
    }
    console.error(
      "[mareader] opened via file:// — serve the app over HTTP instead (npm run dev:frontend)"
    );
    return;
  }

  // Each stage is the sentence for a wait of at least `at` milliseconds. The
  // last one is the deadline the failure report rides on: past it the shell is
  // not starting, and the mark stops (styles/boot.css) because an animation
  // under "the shell did not start" would be lying.
  var STAGES = [
    { at: 4000, text: "Still loading the app itself — no library index or database to build." },
    { at: 9000, text: "Still loading. The browser console names what the shell is waiting for." },
    { at: 20000, text: "The shell did not start. The browser console has the details." },
  ];
  var DEADLINE_MS = STAGES[STAGES.length - 1].at;

  STAGES.forEach(function (stage) {
    window.setTimeout(function () {
      var boot = document.getElementById("shell-boot");
      // The healthy path: the shell took over and removed the placeholder.
      if (!boot) return;
      var hint = boot.querySelector(".shell-boot__hint");
      if (hint) {
        hint.textContent = stage.text;
      }
      if (stage.at !== DEADLINE_MS) return;
      boot.setAttribute("data-shell-boot", "timeout");
      console.error(
        "[mareader] the shell did not start within " +
          DEADLINE_MS / 1000 +
          "s: check the console for the failure that stopped it"
      );
    }, stage.at);
  });
})();

// only the changed file was rewritten
