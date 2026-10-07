// The shell page's boot screen: the wait's copy, made specific.
(function () {
  // A file:// document can never start: say so at once.
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

  // Each stage is the sentence for a wait of at least `at` ms.
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
