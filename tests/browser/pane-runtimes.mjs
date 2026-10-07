// Real-realm regressions appended to the lifecycle suite, without relaxing
// any existing gate. Screenshots live inside its existing dist artifact.
import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export async function verifyPaneRuntimes(args) {
  try {
    return await run(args);
  } catch (error) {
    const out = fileURLToPath(new URL("../../dist/verification/", import.meta.url));
    const facts = await args.page.evaluate(() => ({
      root: JSON.parse(window.__mareaderDiagnostics()),
      marker: window.__paneVerificationMarker,
      frames: [...[...document.querySelectorAll('iframe[data-mareader-runtime-frame="reader"]')].flatMap((host) => [...(host.contentDocument?.querySelectorAll("iframe.pane-frame") ?? [])])].map((f) => ({
        src: f.src, hidden: f.hasAttribute("data-frame-hidden"),
        slot: f.closest(".runtime-frame")?.dataset.mareaderSlot,
        pane: f.closest("[data-pane-id]")?.dataset.paneId,
        report: JSON.parse(f.contentWindow.__mareaderDiagnostics?.() ?? "null"),
      })),
    })).catch((failure) => ({ captureError: String(failure) }));
    mkdirSync(out, { recursive: true });
    writeFileSync(`${out}/failure.json`, JSON.stringify({ error: String(error), facts }, null, 2));
    await args.page.screenshot({ path: `${out}/failure.png`, fullPage: true }).catch(() => {});
    console.error("PANE_RUNTIME_FAILURE_JSON " + JSON.stringify({ error: String(error), facts }));
    throw error;
  }
}

async function run({ page, openBook, openIn, waitFor, waitForSettledLayout,
  frameClick, writeSettings, closeAndWaitBaseline, snap, urls, paths }) {
  const out = fileURLToPath(new URL("../../dist/verification/", import.meta.url));
  mkdirSync(out, { recursive: true });
  const report = { sha: process.env.GITHUB_SHA ?? null, replacements: [], screenshots: [], appearanceGeometry: [] };
  await writeSettings({ appearance: { base: "light", tintStrength: 0, noise: "off" },
    workspace: { independentThemes: false, sharedBaseMode: true, paneGap: 0 } });
  const initial = await openBook(urls.pdf);
  await page.evaluate(() => Object.defineProperty(window, "__paneReaderDocument", {
    configurable: true,
    get() {
      return document.querySelector('iframe.runtime-frame[data-mareader-slot="active"][data-mareader-runtime-frame="reader"]')?.contentDocument;
    },
  }));
  report.hierarchy = await page.evaluate(() => {
    const host = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"]');
    const panes = [...host.contentDocument.querySelectorAll("iframe.pane-frame")];
    return { routeTag: host.tagName, route: new URL(host.src).pathname,
      children: panes.map((f) => new URL(f.src).pathname),
      shellHasPdf: typeof window.PDFReader !== "undefined",
      hostNoiseOverlays: host.contentDocument.defaultView.document.querySelectorAll(".noise-overlay").length,
      childNoiseOverlays: panes.map((f) => f.contentDocument.querySelectorAll(".noise-overlay").length),
      hostHasPdf: typeof host.contentDocument.defaultView.PDFReader !== "undefined" };
  });
  if (report.hierarchy.routeTag !== "IFRAME" || report.hierarchy.route !== "/reader.html" ||
      report.hierarchy.shellHasPdf || report.hierarchy.hostHasPdf ||
      report.hierarchy.hostNoiseOverlays !== 1 || report.hierarchy.childNoiseOverlays.some((n) => n !== 0) ||
      !report.hierarchy.children.includes("/pdf.html")) {
    throw new Error(`route/document hierarchy broken: ${JSON.stringify(report.hierarchy)}`);
  }
  // Layout-ready is a host lifecycle, not proof that a descendant has
  // finished loading/painting. Scope, lift and close races need real realms.
  const paintedRealms = (ids) => page.waitForFunction((ids) => ids.every((id) => {
    const frames = [...window.__paneReaderDocument.querySelectorAll(`[data-pane-id="${id}"] iframe.pane-frame`)];
    if (frames.length !== 1 || frames[0].hasAttribute("data-frame-hidden")) return false;
    const raw = frames[0].contentWindow.__mareaderDiagnostics?.();
    return raw && JSON.parse(raw).runtime?.state === "ready";
  }), ids, { timeout: 15_000 });
  const pane = initial.host.panes[0].paneId;
  const marker = await page.evaluate(() => {
    window.__paneVerificationMarker = crypto.randomUUID();
    return window.__paneVerificationMarker;
  });
  const visible = (id) => page.evaluate((id) => {
    const f = window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"] iframe.pane-frame:not([data-frame-hidden])`);
    return f ? { src: f.src, path: JSON.parse(f.contentWindow.__mareaderDiagnostics()).host?.panes?.[0]?.documentId,
      hasPdf: typeof f.contentWindow.PDFReader !== "undefined" } : null;
  }, id);
  const replaced = async (path, format) => {
    const before = await visible(pane);
    if ((await openIn(path, "active")) !== true) throw new Error(`replacement refused: ${path}`);
    const s = await waitFor(`fresh ${format} replacement`, (s) => s.host?.panes?.length === 1 &&
      s.host.panes[0].paneId === pane && s.host.panes[0].format === format &&
      s.host.panes[0].documentId === `path:${path}` && s.host.panes[0].lifecycle === "ready");
    await page.waitForFunction(([id, src]) => {
      const frames = [...window.__paneReaderDocument.querySelectorAll(`[data-pane-id="${id}"] iframe.pane-frame`)];
      return frames.length === 1 && frames[0].src !== src && !frames[0].hasAttribute("data-frame-hidden");
    }, [pane, before.src], { timeout: 15_000 });
    const after = await visible(pane);
    if (after.src === before.src) throw new Error(`${format} reused a document-bearing WASM realm`);
    if (await page.evaluate(() => window.__paneVerificationMarker) !== marker) throw new Error("replacement reloaded the Shell");
    if (s.host.panesCreated !== initial.host.panesCreated) throw new Error("replacement recreated its pane identity");
    if (format !== "pdf") {
      const text = await page.evaluate((id) => {
        const w = window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"] iframe.pane-frame`).contentWindow;
        return { engine: typeof w.PDFReader, stats: JSON.parse(w.__mareaderDiagnostics()).engine,
          pdfResources: w.performance.getEntriesByType("resource").filter((e) => /pdfEngine|pdf\.min\.mjs|pdf\.worker/.test(e.name)).length };
      }, pane);
      if (text.engine !== "undefined" || text.stats !== null || text.pdfResources !== 0) {
        throw new Error(`text realm contains PDF machinery: ${JSON.stringify(text)}`);
      }
    }
    report.replacements.push({ format, old: before.src, fresh: after.src });
  };
  // Same-format replacements are as important as crossing the format seam.
  await replaced(paths.otherPdf, "pdf");
  await replaced(paths.markdown, "markdown");
  await replaced(paths.text, "text");

  // Supersede incoming frames before they can adopt. Late hello/paper/outline
  // reports must not replace the final request or leave a boot error over it.
  await page.evaluate((paths) => {
    for (const path of paths) if (!window.__paneReaderDocument.defaultView.__mareaderOpenIn(path, "active")) throw new Error("rapid open refused");
  }, [paths.markdown, paths.pdf, paths.otherPdf]);
  await waitFor("latest rapid replacement only", (s) => s.host?.panes?.length === 1 &&
    s.host.panes[0].documentId === `path:${paths.otherPdf}` && s.host.panes[0].lifecycle === "ready");
  await page.waitForFunction((id) => window.__paneReaderDocument.querySelectorAll(`[data-pane-id="${id}"] iframe.pane-frame`).length === 1, pane);
  if (await page.frameLocator('iframe.runtime-frame[data-mareader-slot="active"]').locator('[data-pane-boot="error"]').count()) throw new Error("a stale startup error survived supersession");
  report.rapidSupersession = true;

  await openIn(paths.pdf, "right");
  const twoPdfs = await waitForSettledLayout("two real PDF realms", (s) => s.host?.panes?.length === 2 && s.host.panes.every((p) => p.lifecycle === "ready"));
  // Host-ready means the iframe exists, not that its document is open. An
  // empty loading frame also has an idle engine, so paint must precede idle.
  await paintedRealms(twoPdfs.host.panes.map((p) => p.paneId));
  await waitFor("two PDF render frontiers idle", (s) => s.engine.pageActive === 0 && s.engine.pageQueue === 0 && s.rasterLane?.active === 0 && s.rasterLane?.queued === 0);
  // Exercise both engines together. These are real full-resolution renders
  // at their existing scale, not fake permit requests or low-resolution covers.
  const budget = await page.evaluate(async () => {
    const frames = [...window.__paneReaderDocument.querySelectorAll('iframe.pane-frame:not([data-frame-hidden])')]
      .filter((f) => typeof f.contentWindow.PDFReader !== "undefined");
    if (frames.length !== 2) throw new Error(`expected two PDF realms, got ${frames.length}`);
    const samples = [];
    let alive = true;
    let raf = 0;
    const sample = () => {
      if (!alive) return;
      const activeRenders = frames.reduce((n, f) => n + f.contentWindow.PDFReader.stats().activeRenders, 0);
      samples.push({ activeRenders, ...window.__mareaderRasterLane.snapshot() });
      if (samples.length > 1024) throw new Error("raster sampler did not settle");
      raf = requestAnimationFrame(sample);
    };
    sample();
    let drainMs = 0;
    let afterFrame = null;
    let drain = null;
    try {
      const jobs = frames.flatMap((f) => [...f.contentDocument.querySelectorAll(".pdf-page canvas[data-engine-sid]")].slice(0, 2).map((canvas) => {
        const scale = Number(f.contentWindow.getComputedStyle(canvas.parentElement).getPropertyValue("--scale-factor")) || 1;
        return f.contentWindow.PDFReader.renderPage(Number(canvas.getAttribute("data-engine-sid")), canvas.id, scale, true);
      }));
      if (jobs.length < 2) throw new Error("the two PDF realms have no registered page jobs");
      const results = await Promise.all(jobs);
      if (results.some((r) => !r.ok)) throw new Error(`cross-realm render failed: ${JSON.stringify(results)}`);
      await new Promise((r) => requestAnimationFrame(r));
      afterFrame = window.__mareaderRasterLane.snapshot();
      // A permit is returned by the REALM that took it, on its own frame, when
      // it applies the result. A parent frame is not that frame: the parent's
      // rAF is sparse while these realms paint, so snapshotting one parent
      // frame after the last job resolves can still catch an in-flight lease.
      // Wait on the lane itself, which the suite's other settle waits allow
      // 30 s to do; 10 s here so a lease that never returns still fails the
      // stage, just loudly and with both snapshots in the message.
      const drainStart = performance.now();
      const deadline = drainStart + 10_000;
      let drained = afterFrame;
      while (drained.active > 0 || drained.queued > 0) {
        if (performance.now() >= deadline) {
          throw new Error(`window raster lane never drained: ${JSON.stringify({ afterFrame, drained })}`);
        }
        await new Promise((r) => setTimeout(r, 16));
        drained = window.__mareaderRasterLane.snapshot();
      }
      drainMs = Math.round(performance.now() - drainStart);
      drain = drained;
    } finally {
      alive = false;
      cancelAnimationFrame(raf);
    }
    if (samples.some((s) => s.active > 2 || s.activeRenders > 2)) throw new Error(`window raster cap exceeded: ${JSON.stringify(samples)}`);
    return { samples: samples.length, peakLeases: Math.max(...samples.map((s) => s.active)),
      peakRenders: Math.max(...samples.map((s) => s.activeRenders)), afterFrame, drainMs,
      final: drain };
  });
  if (budget.peakLeases !== 2 || budget.final.active !== 0 || budget.final.queued !== 0) throw new Error(`unproved window budget: ${JSON.stringify(budget)}`);
  report.rasterBudget = budget;

  await openIn(paths.markdown, "down");
  const mixed = await waitForSettledLayout("three mixed pane realms", (s) => s.host?.panes?.length === 3 && s.host.panes.every((p) => p.lifecycle === "ready"));
  await paintedRealms(mixed.host.panes.map((p) => p.paneId));
  const pdfIds = mixed.host.panes.filter((p) => p.format === "pdf").map((p) => p.paneId);
  const md = mixed.host.panes.find((p) => p.format === "markdown");
  await frameClick('button[title="Appearance"]', "scope menu");
  await frameClick('[role="switch"][title="Independent theme for each pane"]', "enable pane themes");
  await frameClick('button[title="Appearance"]', "close scope menu");
  await waitFor("independent themes settle", (s) => s.engine.activeRenders === 0 && s.rasterLane?.active === 0);
  await page.evaluate((id) => {
    const w = window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"] iframe.pane-frame`).contentWindow;
    w.document.querySelector('[data-pane-root]').dispatchEvent(new w.PointerEvent("pointerdown", { bubbles: true }));
  }, pdfIds[0]);
  await waitFor("scope focuses its PDF pane", (s) => s.host?.activePane === pdfIds[0]);
  await frameClick('button[title="Appearance"]', "scoped slider");
  const scope = await page.evaluate(async (ids) => {
    const windows = ids.map((id) => window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"] iframe.pane-frame`).contentWindow);
    const siblingBefore = windows[1].PDFReader.stats().rendersCompleted;
    const slider = window.__paneReaderDocument.querySelector('input[aria-label="Strength"]');
    if (!slider) throw new Error("appearance strength slider missing");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(slider, "74");
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    return { edited: windows[0].document.documentElement.classList.contains("appearance-scrubbing"),
      sibling: windows[1].document.documentElement.classList.contains("appearance-scrubbing"), siblingBefore };
  }, pdfIds);
  if (!scope.edited || scope.sibling) throw new Error(`scrub crossed its pane scope: ${JSON.stringify(scope)}`);
  await page.waitForFunction((ids) => ids.every((id) => !window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"] iframe.pane-frame`).contentDocument.documentElement.classList.contains("appearance-scrubbing")), pdfIds);
  const siblingAfter = await page.evaluate((id) => window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"] iframe.pane-frame`).contentWindow.PDFReader.stats().rendersCompleted, pdfIds[1]);
  if (siblingAfter !== scope.siblingBefore) throw new Error("a scoped slider rerendered its untouched sibling");
  report.scopedScrub = { ...scope, siblingAfter };
  await frameClick('[role="switch"][title="Independent theme for each pane"]', "restore shared theme");
  await frameClick('button[title="Appearance"]', "close scope menu");

  // A real stationary press on the Markdown gutter, not a synthetic lift.
  const gutter = await page.evaluate((id) => {
    const f = window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"] iframe.pane-frame`);
    const r = f.getBoundingClientRect();
    return { x: r.left + 8, y: r.top + Math.min(180, r.height / 2) };
  }, md.paneId);
  await page.mouse.move(gutter.x, gutter.y);
  const holdStart = Date.now();
  await page.mouse.down();
  try {
    // A press shorter than the lift delay is a pan or a click, never a lift.
    // The probe runs FIRST and off the press's own clock, so nothing later in
    // this block can spend the window and then blame the lift; the delay
    // itself is the product spec (`HOLD_TO_LIFT_MS` in
    // `crates/reader-runtime/src/host/lift.rs`).
    await page.waitForTimeout(400);
    if (await page.frameLocator('iframe.runtime-frame[data-mareader-slot="active"]').locator('.pane-lifted').count()) throw new Error("a press shorter than the lift delay lifted the pane");
    // The ring is the affordance, and it is up for the rest of the hold...
    await page.waitForFunction((id) => window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"] iframe.pane-frame`).contentDocument.querySelector('[data-pan-hold]'), md.paneId, { timeout: 3_000 });
    // ...and the lift lands within a bounded remainder of the spec, not
    // "eventually": the press, the ring and the lift all sit inside 1 s.
    await page.waitForFunction((id) => window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"]`).classList.contains("pane-lifted"), md.paneId, { timeout: 3_000 });
    // The class reacts immediately; the host digest arrives on its beat.
    // Timestamp the lift itself, then require both reported and rendered
    // vacancy growth at a bounded deadline, not a stale immediate digest.
    report.liftHoldMs = Date.now() - holdStart;
    const lifted = await waitFor("lift fills the vacant pane box", (s) =>
      s.host.panes.filter((p) => p.paneId !== md.paneId).some((p) =>
        p.bounds.width > mixed.host.panes.find((old) => old.paneId === p.paneId).bounds.width + 20 ||
        p.bounds.height > mixed.host.panes.find((old) => old.paneId === p.paneId).bounds.height + 20), 5_000);
    await page.waitForFunction((before) => before.some((p) => {
      const el = window.__paneReaderDocument.querySelector(`[data-pane-id="${p.paneId}"]`);
      const r = el?.getBoundingClientRect();
      return r && (r.width > p.bounds.width + 20 || r.height > p.bounds.height + 20);
    }), mixed.host.panes.filter((p) => p.paneId !== md.paneId), { timeout: 5_000 });
    report.liftVacancy = { before: mixed.host.panes.map((p) => ({ id: p.paneId, bounds: p.bounds })),
      after: lifted.host.panes.map((p) => ({ id: p.paneId, bounds: p.bounds })) };
    const dock = await page.evaluate((id) => {
      const r = window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"]`).getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + Math.max(70, r.height * 0.2) };
    }, pdfIds[1]);
    await page.mouse.move(dock.x, dock.y, { steps: 6 });
    await page.waitForTimeout(100);
  } finally {
    await page.mouse.up();
  }
  await page.waitForFunction(() => !window.__paneReaderDocument.querySelector('.pane-lifted'));
  const afterLift = await waitForSettledLayout("lift dock preserves sessions", (s) => s.host?.panes?.length === 3 && s.host.panesCreated === mixed.host.panesCreated && s.engine.sessionsLive === 2);
  if (report.liftHoldMs < 800 || report.liftHoldMs > 1_600) throw new Error(`lift hold outside its 1 s spec: ${report.liftHoldMs}ms`);

  for (const [name, width] of [["desktop", 1400], ["narrow", 640]]) {
    await page.setViewportSize({ width, height: 900 });
    const settled = await waitForSettledLayout(`${name} layout`, (s) => {
      const panes = s.host?.panes ?? [];
      // Old boxes can briefly agree with an old digest after setViewportSize.
      // Agreement must belong to the requested viewport, not the last one.
      return panes.length === 3 && panes.every((p) => p.lifecycle === "ready") &&
        Math.abs(Math.max(...panes.map((p) => p.bounds.x + p.bounds.width)) - width) <= 2 &&
        Math.abs(Math.max(...panes.map((p) => p.bounds.y + p.bounds.height)) - 900) <= 2;
    });
    await waitFor(`${name} rasters settle`, (s) => s.engine.activeRenders === 0 && s.rasterLane?.active === 0);
    // A fit/resize must land the reflow measurements too, not merely the
    // iframe box and PDF work. Otherwise estimated rows can overlap forever.
    await page.waitForFunction(() => {
      const frames = [...window.__paneReaderDocument.querySelectorAll('iframe.pane-frame:not([data-frame-hidden])')]
        .filter((f) => /reflow\.html/.test(f.src));
      return frames.length === 1 && frames.every((f) => {
        const rows = [...f.contentDocument.querySelectorAll('.tx-stream-col > [data-block-index]')];
        return rows.length > 1 && rows.every((row, i) => {
          const rect = row.getBoundingClientRect();
          return rect.height > 0 && (!i || rect.top >= rows[i - 1].getBoundingClientRect().bottom - 2);
        });
      });
    }, null, { timeout: 10_000 });
    // Audit the real Reader document, not the legacy single-pane query
    // adapter. Each route owns its own toolbar IDs and popup coordinates.
    const appearanceButton = page.frameLocator('iframe.runtime-frame[data-mareader-slot="active"][data-mareader-runtime-frame="reader"]').locator('button[title="Appearance"]');
    const hit = await page.evaluate(() => {
      const frame = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"][data-mareader-runtime-frame="reader"]');
      const button = frame.contentDocument.defaultView.document.querySelector('button[title="Appearance"]');
      const r = button.getBoundingClientRect(), outer = frame.getBoundingClientRect();
      return { x: outer.left + r.left + r.width / 2, y: outer.top + r.top + r.height / 2 };
    });
    // Holds prevent an already-revealed toolbar from hiding; they do not
    // reveal it. Geometry must use the same hover/click as an actual user.
    await page.mouse.move(hit.x, hit.y);
    await appearanceButton.click({ timeout: 5_000 });
    const readMenuGeometry = (wait) => {
      const frame = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"][data-mareader-runtime-frame="reader"]');
      const doc = frame?.contentDocument?.defaultView.document;
      const w = doc?.defaultView;
      const button = doc?.querySelector('button[title="Appearance"]');
      const anchor = button?.closest('.relative.inline-flex');
      const panels = [...(doc?.querySelectorAll('.menu-popover') ?? [])];
      const panel = panels[0];
      const row = doc?.querySelector('#toolbar-row');
      if (!anchor || !panel || !row) return wait ? false : null;
      const rect = (el) => {
        const r = el.getBoundingClientRect();
        return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
      };
      const a = rect(anchor), p = rect(panel);
      const clamp = (value, extent, size) => Math.max(8, Math.min(value, extent - size - 8));
      const below = a.bottom + 4 + p.height <= w.innerHeight - 8;
      const expected = { left: clamp(a.right - p.width, w.innerWidth, p.width),
        top: clamp(below ? a.bottom + 4 : a.top - 4 - p.height, w.innerHeight, p.height) };
      const facts = { frame: new URL(frame.src).pathname, toolbarRows: doc.querySelectorAll('#toolbar-row').length,
        shellToolbarRows: document.querySelectorAll('#toolbar-row').length, panels: panels.length,
        sameDocument: panel.ownerDocument === anchor.ownerDocument && panel.ownerDocument === doc,
        anchor: a, panel: p, expected, row: rect(row), rowOpacity: Number(w.getComputedStyle(row).opacity),
        panelOpacity: Number(w.getComputedStyle(panel).opacity), rowInert: row.inert,
        viewport: { width: w.innerWidth, height: w.innerHeight } };
      const aligned = facts.frame === '/reader.html' && facts.toolbarRows === 1 && facts.shellToolbarRows === 0 &&
        facts.panels === 1 && facts.sameDocument && Math.abs(p.width - 288) <= 2 && p.height > 0 &&
        p.left >= 7 && p.right <= w.innerWidth - 7 && p.top >= 7 && p.bottom <= w.innerHeight - 7 &&
        Math.abs(p.left - expected.left) <= 2 && Math.abs(p.top - expected.top) <= 2;
      const visible = facts.rowOpacity > 0.99 && facts.panelOpacity > 0.99 && !facts.rowInert;
      return wait ? aligned && visible : { ...facts, aligned, visible };
    };
    try {
      await page.waitForFunction(readMenuGeometry, true, { timeout: 5_000 });
    } catch (error) {
      const geometry = await page.evaluate(readMenuGeometry, false);
      throw new Error(`${name} appearance menu is not visible/aligned in its Reader document: ${JSON.stringify(geometry)}`, { cause: error });
    }
    const opened = await page.evaluate(readMenuGeometry, false);
    if (!opened?.aligned || !opened.visible) throw new Error(`${name} appearance geometry changed after settling: ${JSON.stringify(opened)}`);
    await page.mouse.move(opened.panel.left + 20, opened.panel.top + 40);
    // Past the real 400 ms hover grace: an open menu must hold the bar even
    // after the pointer leaves the toolbar for the popup's own controls.
    await page.waitForTimeout(650);
    const geometry = await page.evaluate(readMenuGeometry, false);
    if (!geometry?.aligned || !geometry.visible) throw new Error(`${name} appearance menu lost its visible anchor while hovered: ${JSON.stringify(geometry)}`);
    report.appearanceGeometry.push({ name, ...geometry });
    await page.screenshot({ path: `${out}/${name}-appearance.png`, fullPage: true });
    await appearanceButton.click({ timeout: 5_000 });
    await page.waitForFunction(() => !window.__paneReaderDocument.defaultView.document.querySelector('.menu-popover'));
    const facts = await page.evaluate(() => ({ width: innerWidth, overflow: document.documentElement.scrollWidth > innerWidth + 2,
      entries: [...window.__paneReaderDocument.querySelectorAll('[data-pane-id]')].filter((e) => !e.classList.contains("pane-retiring")).length,
      engineFreeText: [...window.__paneReaderDocument.querySelectorAll('iframe.pane-frame:not([data-frame-hidden])')].filter((f) => /reflow\.html/.test(f.src)).every((f) => typeof f.contentWindow.PDFReader === "undefined"),
      textRows: [...window.__paneReaderDocument.querySelector('iframe.pane-frame[src*="reflow.html"]').contentDocument.querySelectorAll('.tx-stream-col > [data-block-index]')].map((row) => {
        const r = row.getBoundingClientRect();
        return { index: Number(row.dataset.blockIndex), top: r.top, bottom: r.bottom };
      }),
      shell: window.__paneVerificationMarker }));
    if (facts.overflow || facts.entries !== 3 || !facts.engineFreeText || facts.shell !== marker) throw new Error(`${name} failed visual-layout facts: ${JSON.stringify(facts)}`);
    await page.screenshot({ path: `${out}/${name}.png`, fullPage: true });
    report.screenshots.push({ name, width, facts, panes: settled.host.panes.map((p) => ({ id: p.paneId, bounds: p.bounds })) });
  }
  await page.setViewportSize({ width: 1400, height: 900 });
  // ── The toolbar's zoom and the keyboard, on a pane that is not the caller ──
  // The toolbar is HOST chrome and the zoom is the PANE's: a press has to
  // travel the wire (`Write::ZoomStep`) and land in the pane's own zoom
  // coordinator, which is the one that resolves a step against the window,
  // the mode and the page. Read that off the PDF page host's `--scale-factor`
  // — the same observable the keyboard zoom is proven by — and read the
  // strip's `scrollTop` for the scrolling keys.
  const pdfPane = await page.evaluate(() => {
    const frames = [...window.__paneReaderDocument.querySelectorAll("[data-pane-id] iframe.pane-frame:not([data-frame-hidden])")];
    const frame = frames.find((f) => /pdf\.html/.test(f.src));
    return frame ? Number(frame.closest("[data-pane-id]").dataset.paneId) : null;
  });
  if (pdfPane === null) throw new Error("no visible PDF pane to read the zoom scale from");
  const probe = () => page.evaluate((id) => {
    const frame = window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"] iframe.pane-frame:not([data-frame-hidden])`);
    const pageHost = frame?.contentDocument?.querySelector(".pdf-page canvas[data-engine-sid]:not(.page-snapshot)")?.parentElement;
    const pane = window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"]`);
    // The strip the reader itself names: inside the pane's own root, never a
    // document-wide first match. `strips` and `travel` are there to explain a
    // strip that does not move — a twin id, or a strip with no room left.
    const strips = frame?.contentDocument?.querySelectorAll("#page-list") ?? [];
    const strip = frame?.contentDocument?.querySelector("[data-pane-root] #page-list") ?? strips[0];
    return { active: pane?.getAttribute("data-pane-active") ?? null,
      strips: strips.length,
      scale: frame && pageHost ? Number(frame.contentWindow.getComputedStyle(pageHost).getPropertyValue("--scale-factor")) || 0 : 0,
      scrollTop: strip?.scrollTop ?? null,
      travel: strip ? strip.scrollHeight - strip.clientHeight : null };
  }, pdfPane);
  const settle = async (label, predicate) => {
    const started = Date.now();
    for (;;) {
      const facts = await probe();
      if (predicate(facts)) return facts;
      if (Date.now() - started > 20_000) {
        // A key the reader did not act on is a fact about where it landed, so
        // a failed settle reports the keys seen and the target each had.
        const keys = await page.evaluate(() => window.__paneKeys ?? null).catch(() => null);
        throw new Error(`${label}: last ${JSON.stringify(facts)}${keys ? ` keys ${JSON.stringify(keys)}` : ""}`);
      }
      await page.waitForTimeout(80);
    }
  };
  // A press inside the pane is what makes it the host's active pane, and the
  // forwarded keys only carry to that one — so the sequence starts where a
  // reader's does. The aim is the pane's middle: the Reader's title bar lies
  // over the top of the workspace and takes the presses there (the pane
  // entry keeps its corner controls below it for exactly that reason), so a
  // press near the pane's corner would land on the chrome and never reach
  // the pane.
  const press = await page.evaluate((id) => {
    const reader = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"][data-mareader-runtime-frame="reader"]');
    const frame = reader.contentDocument.querySelector(`[data-pane-id="${id}"] iframe.pane-frame:not([data-frame-hidden])`);
    const box = frame.getBoundingClientRect(), outer = reader.getBoundingClientRect();
    return { x: outer.left + box.left + box.width / 2, y: outer.top + box.top + box.height / 2 };
  }, pdfPane);
  await page.mouse.click(press.x, press.y);
  const seated = await settle("the pressed PDF pane to take the host's focus", (f) => f.active === "true" && f.scale > 0);
  // The toolbar buttons are clicked on the element (their handler, not a hit
  // test): the bar sits under the window drag region in a packaged app, which
  // this suite already works around for the close button.
  const chromeClick = (title) => page.evaluate((title) => {
    const frame = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"][data-mareader-runtime-frame="reader"]');
    const button = frame?.contentDocument?.defaultView.document.querySelector(`button[title="${title}"]`);
    if (!button) throw new Error(`the Reader chrome has no ${title} button`);
    button.click();
  }, title);
  const viewToolsHit = await page.evaluate(() => {
    const frame = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"][data-mareader-runtime-frame="reader"]');
    const button = frame.contentDocument.defaultView.document.querySelector('button[title="View & tools"]');
    const r = button.getBoundingClientRect(), outer = frame.getBoundingClientRect();
    return { x: outer.left + r.left + r.width / 2, y: outer.top + r.top + r.height / 2 };
  });
  await page.mouse.move(viewToolsHit.x, viewToolsHit.y);
  await chromeClick("View & tools");
  await page.waitForFunction(() => !!window.__paneReaderDocument.defaultView.document.querySelector(".menu-popover"));
  await chromeClick("Zoom in (+)");
  const zoomIn = await settle("the toolbar's zoom-in button never landed", (f) => f.scale > seated.scale);
  await chromeClick("Zoom out (-)");
  const zoomOut = await settle("the toolbar's zoom-out button never landed", (f) => f.scale < zoomIn.scale);
  // An open popover owns its keys (a host menu is a typing surface to the key
  // forwarder), so the Cmd/Ctrl combos are checked with it closed.
  await chromeClick("View & tools");
  await page.waitForFunction(() => !window.__paneReaderDocument.defaultView.document.querySelector(".menu-popover"));
  await page.keyboard.press("Control+Equal");
  const ctrlIn = await settle("Ctrl+= never zoomed in", (f) => f.scale > zoomOut.scale);
  await page.keyboard.press("Control+Minus");
  const ctrlOut = await settle("Ctrl+- never zoomed out", (f) => f.scale < ctrlIn.scale);
  // Vim's home row scrolls the strip the arrows do: `j` nudges down (and
  // glides while held), `k` back up. Three things make the check land on
  // that meaning rather than on a neighbour: the pane is put in the
  // vertical-scroll mode the keys scroll in (its own menu button, the same
  // element click the zoom checks use), the menu is closed again — an open
  // popover owns its own keys — and the pane takes the keyboard the way a
  // reader hands it over, with a press on its surface.
  await chromeClick("View & tools");
  await chromeClick("Vertical scroll");
  await chromeClick("View & tools");
  await page.waitForFunction(() => !window.__paneReaderDocument.defaultView.document.querySelector(".menu-popover"));
  await page.mouse.click(press.x, press.y);
  await settle("the pane the keys are about to move to hold the host's focus", (f) => f.active === "true");
  // …and the zoom has to be over. A transaction owns the strip it rescales
  // while it runs — its own landing write re-lands the offset it anchored —
  // so a nudge issued into it is overwritten (ZOOM_ANIM_MS + ZOOM_GRACE_MS,
  // 420 ms). Wait for the scale to HOLD still past that, then hand the keys
  // over; the strip's baseline below is read on the settled side.
  const stable = async (label, field, holdMs) => {
    const started = Date.now();
    let last = null, since = started;
    for (;;) {
      const value = (await probe())[field];
      if (value !== last) { last = value; since = Date.now(); }
      if (Date.now() - since >= holdMs) return value;
      if (Date.now() - started > 20_000) throw new Error(`${label}: ${field} never held still (last ${JSON.stringify(value)})`);
      await page.waitForTimeout(60);
    }
  };
  await stable("the pane's zoom to settle before the vim keys", "scale", 500);
  // Where a key lands decides whether the reader acts on it, so a failed
  // settle below can say what had the focus instead of only that the strip
  // did not move.
  await page.evaluate((id) => {
    const frame = window.__paneReaderDocument.querySelector(`[data-pane-id="${id}"] iframe.pane-frame:not([data-frame-hidden])`);
    const doc = frame.contentDocument;
    window.__paneKeys = [];
    doc.addEventListener("keydown", (ev) => {
      const path = [];
      for (let el = ev.target; el?.nodeType === 1 && el !== doc.documentElement; el = el.parentElement) {
        path.push(`${el.tagName.toLowerCase()}${el.id ? `#${el.id}` : ""}`);
      }
      window.__paneKeys.push({ key: ev.key, active: doc.activeElement?.tagName.toLowerCase(), path });
    }, true);
  }, pdfPane);
  const stripReady = await settle("a scrollable strip to nudge", (f) => f.scrollTop !== null);
  await page.keyboard.press("j");
  const scrolledDown = await settle("j never scrolled the strip down", (f) => f.scrollTop > stripReady.scrollTop);
  await page.keyboard.press("k");
  const scrolledUp = await settle("k never scrolled the strip back up", (f) => f.scrollTop < scrolledDown.scrollTop);
  report.toolbarZoom = { seat: seated.scale, buttonIn: zoomIn.scale, buttonOut: zoomOut.scale,
    ctrlIn: ctrlIn.scale, ctrlOut: ctrlOut.scale };
  report.vimScroll = { from: stripReady.scrollTop, down: scrolledDown.scrollTop, up: scrolledUp.scrollTop };
  const beforeClose = await snap();
  const closed = await closeAndWaitBaseline("pane runtime regressions", false, beforeClose.disposalEpoch + afterLift.host.panes.length);
  if (closed.rasterLane?.active !== 0 || closed.rasterLane?.queued !== 0 || closed.rasterLane?.owners !== 0) throw new Error("closed frames retained host raster leases");
  const residency = await page.evaluate(() => ({
    readers: document.querySelectorAll('iframe[data-mareader-runtime-frame="reader"]').length,
    library: document.querySelector('iframe.runtime-frame[data-mareader-slot="active"]')?.src,
  }));
  if (closed.readerFramesResident !== 0 || closed.paneFramesResident !== 0 ||
      residency.readers !== 0 || !residency.library?.includes("/library.html")) {
    throw new Error(`mixed workspace kept a Reader WASM realm on Library: ${JSON.stringify(residency)}`);
  }
  report.readerHostUnloaded = { ...residency, readerFramesResident: closed.readerFramesResident,
    paneFramesResident: closed.paneFramesResident };
  report.closed = { atBaseline: closed.atBaseline, sessionsOpened: closed.engine.sessionsOpened,
    sessionsDestroyed: closed.engine.sessionsDestroyed, rasterLane: closed.rasterLane };
  await page.frameLocator('iframe.runtime-frame[data-mareader-slot="active"]')
    .locator('.book-title[title*="Programming Pearls"]').first().click();
  await waitFor("fresh Reader ready", (s) => s.bootState === "reader" && s.host?.panes?.length === 1 &&
    s.host.panes[0].lifecycle === "ready" && s.engine.hasDocument);
  await openIn(paths.markdown, "right");
  const beforePending = await waitForSettledLayout("mixed panes before pending close", (s) => s.host?.panes?.length === 2 &&
    s.host.panes.every((p) => p.lifecycle === "ready"));
  await paintedRealms(beforePending.host.panes.map((p) => p.paneId));
  let release;
  const held = new Promise((resolve) => { release = resolve; });
  let sawBlockedText;
  const textBlocked = new Promise((resolve) => { sawBlockedText = resolve; });
  const delayText = async (route) => { sawBlockedText(); await held; await route.continue().catch(() => {}); };
  await page.route("**/reflow_bg.wasm", delayText);
  try {
    await openIn(paths.text, "active");
    await Promise.race([
      textBlocked,
      page.waitForTimeout(5000).then(() => { throw new Error("pending-close proof never held the reflow WASM response"); }),
    ]);
    await page.waitForFunction(() => !!window.__paneReaderDocument.querySelector('iframe.pane-frame[data-frame-hidden]'));
    await frameClick('button[title*="Close this book"]', "close Reader with incoming document");
    const returned = await waitFor("whole mixed Reader plus incoming realm removed", (s) =>
      s.bootState === "library" && s.atBaseline === true && s.readerFramesResident === 0 && s.paneFramesResident === 0, 15_000);
    if (returned.engine.sessionsOpened !== returned.engine.sessionsDestroyed ||
        returned.engine.workersCreated !== returned.engine.workersTerminated ||
        returned.rasterLane.active !== 0 || returned.rasterLane.queued !== 0 || returned.rasterLane.owners !== 0) {
      throw new Error(`pending mixed close did not drain: ${JSON.stringify(returned)}`);
    }
    report.pendingReaderClose = { atBaseline: true, readerFramesResident: returned.readerFramesResident,
      paneFramesResident: returned.paneFramesResident, sessionsOpened: returned.engine.sessionsOpened,
      sessionsDestroyed: returned.engine.sessionsDestroyed, rasterLane: returned.rasterLane };
  } finally {
    release();
    await page.unroute("**/reflow_bg.wasm", delayText);
  }
  await page.waitForTimeout(500);
  const late = await snap();
  if (late.readerFramesResident !== 0 || late.paneFramesResident !== 0 ||
      await page.evaluate(() => window.__paneVerificationMarker) !== marker) {
    throw new Error("a late pane boot survived or the Shell reloaded on Library return");
  }
  report.libraryScreenshots = [];
  for (const [name, width] of [["desktop", 1400], ["narrow", 640]]) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForFunction((width) => {
      const frame = document.querySelector('iframe[data-mareader-runtime-frame="library"][data-mareader-slot="active"]');
      const grid = frame?.contentDocument?.querySelector('.lib-grid');
      if (!grid) return false;
      const doc = grid.ownerDocument;
      // Viewport dimensions change before the Library's resize observers
      // finish replacing the toolbar's measured center/search width.
      return Math.abs(doc.defaultView.innerWidth - width) <= 2 &&
        grid.getBoundingClientRect().width > 0 &&
        doc.documentElement.scrollWidth <= doc.defaultView.innerWidth + 2;
    }, width, { timeout: 10_000 });
    const fresh = await page.evaluate(() => {
      const frame = document.querySelector('iframe[data-mareader-runtime-frame="library"][data-mareader-slot="active"]');
      const doc = frame.contentDocument.defaultView.document;
      return { generation: Number(frame.dataset.mareaderGeneration), src: frame.src,
        libraryFrames: document.querySelectorAll('iframe[data-mareader-runtime-frame="library"]').length,
        readerFrames: document.querySelectorAll('iframe[data-mareader-runtime-frame="reader"]').length,
        paneFrames: doc.querySelectorAll('iframe.pane-frame').length,
        overflow: doc.documentElement.scrollWidth > doc.defaultView.innerWidth + 2,
        viewportWidth: doc.defaultView.innerWidth, scrollWidth: doc.documentElement.scrollWidth,
        shell: window.__paneVerificationMarker };
    });
    if (fresh.libraryFrames !== 1 || fresh.readerFrames !== 0 || fresh.paneFrames !== 0 || fresh.overflow || fresh.shell !== marker) {
      throw new Error(`${name} fresh Library contains stale realms or overflow: ${JSON.stringify(fresh)}`);
    }
    await page.screenshot({ path: `${out}/library-${name}.png`, fullPage: true });
    report.libraryScreenshots.push({ name, width, ...fresh });
  }
  await page.setViewportSize({ width: 1400, height: 900 });
  writeFileSync(`${out}/pane-runtimes.json`, JSON.stringify(report, null, 2));
  console.log("PANE_RUNTIME_VERIFICATION_JSON " + JSON.stringify(report));
  return report;
}
