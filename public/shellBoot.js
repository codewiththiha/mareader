// The shell page's watchdog: the "never waits forever" half of the boot
// contract.
//
// `#shell-boot` (index.html) is the placeholder the user sees before the shell
// wasm exists. The shell removes it as soon as the runtime host paints its
// first state. This script covers the case where that never happens at all —
// the shell wasm failed to download, or it panicked before its first mount —
// because an infinite "Loading MAReader…" is exactly as uninformative as a
// blank window, and it was the second half of the reported boot bug.
//
// A classic external script, not inline: the packaged app's CSP is
// `script-src 'self' 'wasm-unsafe-eval'` (src-tauri/tauri.conf.json), which
// blocks inline scripts. Copied to the dist root by index.html's
// `data-trunk rel="copy-file"` and required by tools/check-runtime-artifacts.mjs.
(function () {
  var DEADLINE_MS = 20000;

  window.setTimeout(function () {
    var boot = document.getElementById("shell-boot");
    // The healthy path: the shell took over and removed the placeholder.
    if (!boot) return;
    boot.setAttribute("data-shell-boot", "timeout");
    var hint = boot.querySelector(".shell-boot__hint");
    if (hint) {
      hint.textContent =
        "The shell did not start. The browser console has the details.";
    }
    console.error(
      "[mareader] the shell did not start within " +
        DEADLINE_MS / 1000 +
        "s: check the console for the failure that stopped it"
    );
  }, DEADLINE_MS);
})();
