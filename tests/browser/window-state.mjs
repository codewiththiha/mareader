// Real Library WASM against a deterministic Tauri window/event boundary.
// This checks the compiled titlebar wiring, not an orphaned helper's source.
export async function verifyWindowState({ browser, base }) {
  const context = await browser.newContext({ viewport: { width: 1000, height: 800 } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(String(error)));
  try {
    await context.addInitScript(() => {
      const generation = new URL(location.href).searchParams.get("g");
      if (window === window.top || !["1", "3"].includes(generation)) return;
      // A route frame runs no repeating timer: its cadence is observers and
      // rAF. Counting creations in this realm measures what the mounted titlebar
      // itself does — the host's channel ticker lives in another window.
      window.__intervalCreations = 0;
      const setInterval = window.setInterval.bind(window);
      window.setInterval = (...args) => {
        window.__intervalCreations += 1;
        return setInterval(...args);
      };
      if (generation !== "3") return;
      // Generation 3 has no Tauri surface and never gains one before it is
      // disposed: the relay publishes from this page's own <head>, ahead of the
      // module script, so a bar that finds nothing there is a plain browser.
      let surface;
      let available = false;
      Object.defineProperty(window, "__TAURI__", {
        configurable: true,
        get: () => available ? surface : undefined,
        set: (value) => { surface = value; },
      });
      window.__releaseWindowSurface = () => { available = true; };
    });
    // This existing same-origin page is inert when opened top-level; no
    // Shell/runtime starts underneath the manually hosted Library fixture.
    await page.goto(`${base}/bake.html`, { waitUntil: "load" });
    await page.evaluate((base) => {
      const native = window.__windowProbe = {
        maximized: true, queries: 0, delayQuery: false, delayedQueries: [],
        holdResize: false, delayedRegistrations: [], resizeHandlers: new Set(), unlistens: 0,
      };
      native.emit = () => { for (const handler of native.resizeHandlers) handler({ payload: null }); };
      const handle = {
        async isMaximized() {
          native.queries += 1;
          if (native.delayQuery) return new Promise((resolve) => native.delayedQueries.push(resolve));
          return native.maximized;
        },
        async toggleMaximize() { native.maximized = !native.maximized; native.emit(); },
        async minimize() {}, async close() {},
      };
      window.__TAURI__ = {
        core: { async invoke() { return null; } },
        window: { getCurrentWindow: () => handle },
        event: { listen(name, handler) {
          if (name !== "tauri://resize") return Promise.resolve(() => {});
          native.resizeHandlers.add(handler);
          const unlisten = () => { native.resizeHandlers.delete(handler); native.unlistens += 1; };
          if (native.holdResize) return new Promise((resolve) => native.delayedRegistrations.push(() => resolve(unlisten)));
          return Promise.resolve(unlisten);
        } },
      };
      window.__mountWindowProbe = (generation) => {
        const nonce = `window-probe-${generation}`;
        const frame = document.createElement("iframe");
        frame.id = "window-probe-frame";
        frame.style.cssText = "width:100%;height:750px;border:0";
        frame.src = `${base}/library.html?hosted=1&g=${generation}&n=${nonce}`;
        const fixture = window.__windowFixture = { generation, nonce, ready: false, disposed: false, port: null, offers: [] };
        const offer = () => {
          const channel = new MessageChannel();
          fixture.offers.push(channel.port1);
          channel.port1.onmessage = (event) => {
            const message = JSON.parse(event.data);
            if (message.kind === "status" && message.stage === "initialized") {
              fixture.port = channel.port1;
              clearInterval(fixture.ticker);
              for (const other of fixture.offers) if (other !== fixture.port) other.close();
              fixture.offers = [fixture.port];
              fixture.port.postMessage({ generation, nonce, kind: "init", runtime: "library", launch: null, hidden: false });
            }
            if (message.kind === "ready") fixture.ready = true;
            if (message.kind === "disposeComplete") fixture.disposed = true;
          };
          frame.contentWindow.postMessage({ kind: "mareader.channel", generation, nonce }, location.origin, [channel.port2]);
        };
        frame.onload = offer;
        // Trunk's async init can outlive document load; never mistake a
        // missed early offer for missing titlebar wiring.
        fixture.ticker = setInterval(offer, 300);
        document.body.append(frame);
      };
      window.__disposeWindowProbe = () => {
        const f = window.__windowFixture;
        f.port.postMessage({ generation: f.generation, nonce: f.nonce, kind: "dispose" });
      };
      window.__mountWindowProbe(1);
    }, base);
    const captions = () => page.frameLocator("#window-probe-frame");
    const frameStats = () =>
      page.evaluate(() => ({
        queries: window.__windowProbe.queries,
        handlers: window.__windowProbe.resizeHandlers.size,
        intervals: document.getElementById("window-probe-frame").contentWindow.__intervalCreations,
      }));
    await page.waitForFunction(() => window.__windowFixture.ready, null, { timeout: 30_000 });
    await captions().locator('button[aria-label="Restore"]').waitFor({ state: "attached" });
    await page.waitForFunction(() => window.__windowProbe.resizeHandlers.size === 1);
    const before = await page.evaluate(() => window.__windowProbe.queries);
    await page.evaluate(() => {
      const native = window.__windowProbe;
      native.maximized = false;
      native.delayQuery = true;
      for (let i = 0; i < 30; i += 1) native.emit();
    });
    await page.waitForFunction((before) => window.__windowProbe.queries === before + 1, before);
    await page.evaluate(() => {
      const native = window.__windowProbe;
      native.delayQuery = false;
      for (const resolve of native.delayedQueries.splice(0)) resolve(native.maximized);
    });
    await captions().locator('button[aria-label="Maximize"]').waitFor({ state: "attached" });
    await page.waitForFunction((before) => window.__windowProbe.queries === before + 2, before);
    const stormQueries = await page.evaluate((before) => window.__windowProbe.queries - before, before);
    await page.evaluate(() => { window.__windowProbe.maximized = true; window.__windowProbe.emit(); });
    await captions().locator('button[aria-label="Restore"]').waitFor({ state: "attached" });
    await page.evaluate(() => window.__disposeWindowProbe());
    await page.waitForFunction(() => window.__windowFixture.disposed && window.__windowProbe.resizeHandlers.size === 0);
    await page.evaluate(() => {
      document.getElementById("window-probe-frame").remove();
      window.__windowFixture.port.close();
      window.__windowProbe.holdResize = true;
      window.__mountWindowProbe(2);
    });
    await page.waitForFunction(() => window.__windowFixture.ready && window.__windowProbe.delayedRegistrations.length === 1);
    await captions().locator('button[aria-label="Restore"]').waitFor({ state: "attached" });
    await page.evaluate(() => window.__disposeWindowProbe());
    await page.waitForFunction(() => window.__windowFixture.disposed);
    const retiredQueries = await page.evaluate(() => {
      const native = window.__windowProbe;
      const before = native.queries;
      native.emit(); // Native registration is still pending, but handler is inert.
      for (const complete of native.delayedRegistrations.splice(0)) complete();
      return before;
    });
    await page.waitForFunction(() => window.__windowProbe.resizeHandlers.size === 0 && window.__windowProbe.unlistens === 2);
    await page.evaluate(() => {
      document.getElementById("window-probe-frame").remove();
      window.__windowFixture.port.close();
      window.__windowProbe.holdResize = false;
      window.__mountWindowProbe(3);
    });
    await page.waitForFunction(() => window.__windowFixture.ready);
    await captions().locator('button[aria-label="Maximize"]').waitFor({ state: "attached" });
    // A bar with no window reaches for nothing: the probe declines, no listener
    // registers, and no timer is created to wait for a surface that cannot
    // arrive. `intervals` is counted in this realm only — a poll there holds the
    // route's `Owner`, and a frame whose runtime never reports disposal is the
    // failure that timer produced. `queries` is the host's counter shared by
    // every generation, so it is compared with itself: what matters is that an
    // idle surface-less route adds nothing to it.
    const quiet = await frameStats();
    if (quiet.intervals !== 0 || quiet.handlers !== 0) {
      throw new Error(`a surface-less route registered for a window it cannot reach: ${JSON.stringify(quiet)}`);
    }
    await page.waitForTimeout(750);
    const idle = await frameStats();
    if (idle.intervals !== 0 || idle.handlers !== 0 || idle.queries !== quiet.queries) {
      throw new Error(`a mounted titlebar polls for Tauri instead of declining: ${JSON.stringify({ quiet, idle })}`);
    }
    if (await captions().locator('button[aria-label="Restore"]').count() !== 0) {
      throw new Error("a frame with no window invented a maximized state");
    }
    await page.evaluate(() => window.__disposeWindowProbe());
    await page.waitForFunction(() => window.__windowFixture.disposed);
    await page.evaluate(() => document.getElementById("window-probe-frame").contentWindow.__releaseWindowSurface());
    // Keep the disposed realm alive beyond two retry ticks to expose a leak.
    await page.waitForTimeout(750);
    const final = await page.evaluate(() => ({ queries: window.__windowProbe.queries, handlers: window.__windowProbe.resizeHandlers.size }));
    if (final.queries !== retiredQueries || final.handlers !== 0 || errors.length) throw new Error(`disposed window bridge ran: ${JSON.stringify({ final, retiredQueries, errors })}`);
    return { initialMaximized: true, externalResizeUpdates: true, stormQueries,
      surfacelessRouteIdle: true, disposedBeforeSurfaceSuppressed: true,
      pendingRegistrationUnlistened: true, retiredProbeSuppressed: true };
  } finally {
    await context.close();
  }
}

