// Production path checks for the cleanup: async Shell resolution and no
// prebooted empty document realm. Access real documents, not query adapters.
export async function verifyCleanupRuntime({ page, browser, base, paths, openBook, waitFor, snap }) {
  await openBook(`${base}/?open=${encodeURIComponent(paths.pdf)}`);
  await page.waitForFunction((path) => {
    const blob = JSON.parse(localStorage.getItem("mareader.library.v3") ?? "null");
    return blob?.books?.some((row) => (row.origin?.store ?? row.origin?.src) === path);
  }, paths.pdf, { timeout: 5_000 });
  const book = await page.evaluate((path) => {
    const blob = JSON.parse(localStorage.getItem("mareader.library.v3"));
    return blob.books.find((row) => (row.origin?.store ?? row.origin?.src) === path);
  }, paths.pdf);
  if (!book?.id) throw new Error("launch-resolution proof has no stored book identity");
  const reader = 'iframe.runtime-frame[data-mareader-slot="active"][data-mareader-runtime-frame="reader"]';
  const initialRealm = await page.evaluate((reader) => {
    const doc = document.querySelector(reader).contentDocument.defaultView.document;
    return doc.querySelector('iframe.pane-frame:not([data-frame-hidden])').src;
  }, reader);
  const dialogOpen = async () => {
    const surface = page.frameLocator(reader).frameLocator('iframe.pane-frame:not([data-frame-hidden])').locator('[data-pane-root]');
    // The wrapper is not a tab stop. A real click enters its browsing
    // context before keyboard input; press() on the div alone cannot.
    await surface.click({ position: { x: 8, y: 170 } });
    const calls = await page.evaluate(() => window.__cleanupDialogCalls ?? 0);
    await page.evaluate(([reader, path]) => {
      const doc = document.querySelector(reader).contentDocument.defaultView.document;
      const w = doc.querySelector('iframe.pane-frame:not([data-frame-hidden])').contentWindow;
      w.__TAURI__ = { dialog: { open() {
        window.__cleanupDialogCalls = (window.__cleanupDialogCalls ?? 0) + 1;
        // The dialog is the only native surface mocked. Remove it before
        // the replacement realm boots, so real HTTP document IO is unchanged.
        w.queueMicrotask(() => { delete w.__TAURI__; });
        return Promise.resolve(path);
      } } };
    }, [reader, paths.pdf]);
    await page.keyboard.press("Control+o");
    await page.waitForFunction((calls) => window.__cleanupDialogCalls === calls + 1, calls, { timeout: 5_000 });
  };
  await dialogOpen();
  await waitFor("dialog path resolved through Shell", (s) => s.host?.panes?.[0]?.documentId === `book:${book.id}` && s.engine?.hasDocument, 15_000);
  await page.waitForFunction(([reader, old]) => {
    const doc = document.querySelector(reader).contentDocument.defaultView.document;
    const frames = [...doc.querySelectorAll('iframe.pane-frame:not([data-frame-hidden])')];
    return frames.length === 1 && frames[0].src !== old;
  }, [reader, initialRealm]);
  const beforeHeld = await snap();
  await page.evaluate(() => {
    const original = MessagePort.prototype.postMessage;
    window.__restoreLaunchReplies = () => { MessagePort.prototype.postMessage = original; };
    window.__heldLaunchReplies = [];
    MessagePort.prototype.postMessage = function (data, ...rest) {
      if (data?.kind === "resolveLaunchAnswer") {
        window.__heldLaunchReplies.push(() => original.call(this, data, ...rest));
        return;
      }
      return original.call(this, data, ...rest);
    };
  });
  try {
    await dialogOpen();
    await page.waitForFunction(() => window.__heldLaunchReplies.length === 1, null, { timeout: 5_000 });
    await page.frameLocator(reader).locator('button[title*="Close this book"]')
      .evaluate((button) => button.click());
    await waitFor("dispose during launch resolution", (s) => s.activeRuntime === "library" && s.atBaseline &&
      s.readerFramesResident === 0 && s.readerDisposesCompleted > beforeHeld.readerDisposesCompleted, 15_000);
    await page.evaluate(() => {
      window.__restoreLaunchReplies();
      for (const reply of window.__heldLaunchReplies.splice(0)) {
        try { reply(); } catch { /* The retired port is closed. */ }
      }
    });
    await page.waitForTimeout(500);
    const late = await snap();
    if (!late.atBaseline || late.readerFramesResident !== 0 || late.paneFramesResident !== 0) {
      throw new Error(`late resolved launch revived Reader: ${JSON.stringify(late)}`);
    }
  } finally {
    await page.evaluate(() => {
      window.__restoreLaunchReplies?.();
      delete window.__restoreLaunchReplies;
      delete window.__heldLaunchReplies;
    });
  }

  const context = await browser.newContext({ viewport: { width: 1000, height: 800 } });
  const empty = await context.newPage();
  try {
    await empty.goto(`${base}/reader.html`, { waitUntil: "load" });
    await empty.waitForFunction(() => window.__mareaderOpenIn && JSON.parse(window.__mareaderDiagnostics()).host?.panes?.length === 1);
    const idle = await empty.evaluate(() => ({
      frames: document.querySelectorAll('iframe.pane-frame').length,
      resources: performance.getEntriesByType("resource").filter((r) => /\/(pdf|reflow)(\.html|\.js|_bg\.wasm)(?:$|[?#])/.test(r.name)).length,
    }));
    if (idle.frames !== 0 || idle.resources !== 0) throw new Error(`empty host prebooted a document runtime: ${JSON.stringify(idle)}`);
    await empty.evaluate((path) => {
      if (!window.__mareaderOpenIn(path, "active")) throw new Error("documentless host refused its first open");
    }, paths.text);
    await empty.waitForFunction(() => {
      const frame = document.querySelector('iframe.pane-frame:not([data-frame-hidden])');
      return frame && JSON.parse(frame.contentWindow.__mareaderDiagnostics()).runtime.state === "ready";
    });
    if (await empty.locator('iframe.pane-frame').count() !== 1) throw new Error("first actual open created extra realms");
    return { resolvedBookIdentity: book.id, lateResolveIgnored: true, emptyHostFrames: idle.frames,
      emptyHostDocumentFetches: idle.resources, firstOpenRealms: 1 };
  } finally {
    await context.close();
  }
}
