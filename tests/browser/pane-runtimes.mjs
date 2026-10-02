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
      frames: [...document.querySelectorAll("iframe.pane-frame")].map((f) => ({
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
  const report = { sha: process.env.GITHUB_SHA ?? null, replacements: [], screenshots: [] };
  await writeSettings({ appearance: { base: "light", tintStrength: 0, noise: "off" },
    workspace: { independentThemes: false, sharedBaseMode: true, paneGap: 0 } });
  const initial = await openBook(urls.pdf);
  const pane = initial.host.panes[0].paneId;
  const marker = await page.evaluate(() => {
    window.__paneVerificationMarker = crypto.randomUUID();
    return window.__paneVerificationMarker;
  });
  const visible = (id) => page.evaluate((id) => {
    const f = document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"] iframe.pane-frame:not([data-frame-hidden])`);
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
      const frames = [...document.querySelectorAll(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"] iframe.pane-frame`)];
      return frames.length === 1 && frames[0].src !== src && !frames[0].hasAttribute("data-frame-hidden");
    }, [pane, before.src], { timeout: 15_000 });
    const after = await visible(pane);
    if (after.src === before.src) throw new Error(`${format} reused a document-bearing WASM realm`);
    if (await page.evaluate(() => window.__paneVerificationMarker) !== marker) throw new Error("replacement reloaded the Shell");
    if (s.host.panesCreated !== initial.host.panesCreated) throw new Error("replacement recreated its pane identity");
    if (format !== "pdf") {
      const text = await page.evaluate((id) => {
        const w = document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"] iframe.pane-frame`).contentWindow;
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
    for (const path of paths) if (!window.__mareaderOpenIn(path, "active")) throw new Error("rapid open refused");
  }, [paths.markdown, paths.pdf, paths.otherPdf]);
  await waitFor("latest rapid replacement only", (s) => s.host?.panes?.length === 1 &&
    s.host.panes[0].documentId === `path:${paths.otherPdf}` && s.host.panes[0].lifecycle === "ready");
  await page.waitForFunction((id) => document.querySelectorAll(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"] iframe.pane-frame`).length === 1, pane);
  if (await page.locator('[data-pane-boot="error"]').count()) throw new Error("a stale startup error survived supersession");
  report.rapidSupersession = true;

  await openIn(paths.pdf, "right");
  await waitForSettledLayout("two real PDF realms", (s) => s.host?.panes?.length === 2 && s.host.panes.every((p) => p.lifecycle === "ready"));
  await waitFor("two PDF render frontiers idle", (s) => s.engine.pageActive === 0 && s.engine.pageQueue === 0 && s.rasterLane?.active === 0 && s.rasterLane?.queued === 0);
  // Exercise both engines together. These are real full-resolution renders
  // at their existing scale, not fake permit requests or low-resolution covers.
  const budget = await page.evaluate(async () => {
    const frames = [...document.querySelectorAll('#runtime-host .runtime-frame[data-mareader-slot="active"] iframe.pane-frame:not([data-frame-hidden])')]
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
    try {
      const jobs = frames.flatMap((f) => [...f.contentDocument.querySelectorAll(".pdf-page canvas[data-engine-sid]")].slice(0, 2).map((canvas) => {
        const scale = Number(f.contentWindow.getComputedStyle(canvas.parentElement).getPropertyValue("--scale-factor")) || 1;
        return f.contentWindow.PDFReader.renderPage(Number(canvas.getAttribute("data-engine-sid")), canvas.id, scale, true);
      }));
      if (jobs.length < 2) throw new Error("the two PDF realms have no registered page jobs");
      const results = await Promise.all(jobs);
      if (results.some((r) => !r.ok)) throw new Error(`cross-realm render failed: ${JSON.stringify(results)}`);
      await new Promise((r) => requestAnimationFrame(r));
    } finally {
      alive = false;
      cancelAnimationFrame(raf);
    }
    if (samples.some((s) => s.active > 2 || s.activeRenders > 2)) throw new Error(`window raster cap exceeded: ${JSON.stringify(samples)}`);
    return { samples: samples.length, peakLeases: Math.max(...samples.map((s) => s.active)),
      peakRenders: Math.max(...samples.map((s) => s.activeRenders)), final: window.__mareaderRasterLane.snapshot() };
  });
  if (budget.peakLeases !== 2 || budget.final.active !== 0 || budget.final.queued !== 0) throw new Error(`unproved/draining window budget: ${JSON.stringify(budget)}`);
  report.rasterBudget = budget;

  await openIn(paths.markdown, "down");
  const mixed = await waitForSettledLayout("three mixed pane realms", (s) => s.host?.panes?.length === 3 && s.host.panes.every((p) => p.lifecycle === "ready"));
  const pdfIds = mixed.host.panes.filter((p) => p.format === "pdf").map((p) => p.paneId);
  const md = mixed.host.panes.find((p) => p.format === "markdown");
  await frameClick('button[title="Appearance"]', "scope menu");
  await frameClick('[role="switch"][title="Independent theme for each pane"]', "enable pane themes");
  await frameClick('button[title="Appearance"]', "close scope menu");
  await waitFor("independent themes settle", (s) => s.engine.activeRenders === 0 && s.rasterLane?.active === 0);
  await page.evaluate((id) => {
    const w = document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"] iframe.pane-frame`).contentWindow;
    w.document.querySelector('[data-pane-root]').dispatchEvent(new w.PointerEvent("pointerdown", { bubbles: true }));
  }, pdfIds[0]);
  await waitFor("scope focuses its PDF pane", (s) => s.host?.activePane === pdfIds[0]);
  await frameClick('button[title="Appearance"]', "scoped slider");
  const scope = await page.evaluate(async (ids) => {
    const windows = ids.map((id) => document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"] iframe.pane-frame`).contentWindow);
    const siblingBefore = windows[1].PDFReader.stats().rendersCompleted;
    const slider = document.querySelector('input[aria-label="Strength"]');
    if (!slider) throw new Error("appearance strength slider missing");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(slider, "74");
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    return { edited: windows[0].document.documentElement.classList.contains("appearance-scrubbing"),
      sibling: windows[1].document.documentElement.classList.contains("appearance-scrubbing"), siblingBefore };
  }, pdfIds);
  if (!scope.edited || scope.sibling) throw new Error(`scrub crossed its pane scope: ${JSON.stringify(scope)}`);
  await page.waitForFunction((ids) => ids.every((id) => !document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"] iframe.pane-frame`).contentDocument.documentElement.classList.contains("appearance-scrubbing")), pdfIds);
  const siblingAfter = await page.evaluate((id) => document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"] iframe.pane-frame`).contentWindow.PDFReader.stats().rendersCompleted, pdfIds[1]);
  if (siblingAfter !== scope.siblingBefore) throw new Error("a scoped slider rerendered its untouched sibling");
  report.scopedScrub = { ...scope, siblingAfter };
  await frameClick('[role="switch"][title="Independent theme for each pane"]', "restore shared theme");
  await frameClick('button[title="Appearance"]', "close scope menu");

  // A real stationary press on the Markdown gutter, not a synthetic lift.
  const gutter = await page.evaluate((id) => {
    const f = document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"] iframe.pane-frame`);
    const r = f.getBoundingClientRect();
    return { x: r.left + 8, y: r.top + Math.min(180, r.height / 2) };
  }, md.paneId);
  await page.mouse.move(gutter.x, gutter.y);
  const holdStart = Date.now();
  await page.mouse.down();
  try {
    await page.waitForFunction((id) => document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"] iframe.pane-frame`).contentDocument.querySelector('[data-pan-hold]'), md.paneId, { timeout: 3_000 });
    await page.waitForTimeout(2500);
    if (await page.locator('#runtime-host .runtime-frame[data-mareader-slot="active"] .pane-lifted').count()) throw new Error("a short hold lifted the pane before five seconds");
    await page.waitForFunction((id) => document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"]`).classList.contains("pane-lifted"), md.paneId, { timeout: 6_000 });
    // The class reacts immediately; the host digest arrives on its beat.
    // Timestamp the lift itself, then require both reported and rendered
    // vacancy growth at a bounded deadline, not a stale immediate digest.
    report.liftHoldMs = Date.now() - holdStart;
    const lifted = await waitFor("lift fills the vacant pane box", (s) =>
      s.host.panes.filter((p) => p.paneId !== md.paneId).some((p) =>
        p.bounds.width > mixed.host.panes.find((old) => old.paneId === p.paneId).bounds.width + 20 ||
        p.bounds.height > mixed.host.panes.find((old) => old.paneId === p.paneId).bounds.height + 20), 5_000);
    await page.waitForFunction((before) => before.some((p) => {
      const el = document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${p.paneId}"]`);
      const r = el?.getBoundingClientRect();
      return r && (r.width > p.bounds.width + 20 || r.height > p.bounds.height + 20);
    }), mixed.host.panes.filter((p) => p.paneId !== md.paneId), { timeout: 5_000 });
    report.liftVacancy = { before: mixed.host.panes.map((p) => ({ id: p.paneId, bounds: p.bounds })),
      after: lifted.host.panes.map((p) => ({ id: p.paneId, bounds: p.bounds })) };
    const dock = await page.evaluate((id) => {
      const r = document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id="${id}"]`).getBoundingClientRect();
      return { x: r.left + r.width / 2, y: r.top + Math.max(70, r.height * 0.2) };
    }, pdfIds[1]);
    await page.mouse.move(dock.x, dock.y, { steps: 6 });
    await page.waitForTimeout(100);
  } finally {
    await page.mouse.up();
  }
  await page.waitForFunction(() => !document.querySelector('#runtime-host .runtime-frame[data-mareader-slot="active"] .pane-lifted'));
  const afterLift = await waitForSettledLayout("lift dock preserves sessions", (s) => s.host?.panes?.length === 3 && s.host.panesCreated === mixed.host.panesCreated && s.engine.sessionsLive === 2);
  if (report.liftHoldMs < 4800) throw new Error(`lift completed too soon: ${report.liftHoldMs}ms`);

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
      const frames = [...document.querySelectorAll('#runtime-host .runtime-frame[data-mareader-slot="active"] iframe.pane-frame:not([data-frame-hidden])')]
        .filter((f) => /reflow\.html/.test(f.src));
      return frames.length === 1 && frames.every((f) => {
        const rows = [...f.contentDocument.querySelectorAll('.tx-stream-col > [data-block-index]')];
        return rows.length > 1 && rows.every((row, i) => {
          const rect = row.getBoundingClientRect();
          return rect.height > 0 && (!i || rect.top >= rows[i - 1].getBoundingClientRect().bottom - 2);
        });
      });
    }, null, { timeout: 10_000 });
    const facts = await page.evaluate(() => ({ width: innerWidth, overflow: document.documentElement.scrollWidth > innerWidth + 2,
      entries: [...document.querySelectorAll('#runtime-host .runtime-frame[data-mareader-slot="active"] [data-pane-id]')].filter((e) => !e.classList.contains("pane-retiring")).length,
      engineFreeText: [...document.querySelectorAll('#runtime-host .runtime-frame[data-mareader-slot="active"] iframe.pane-frame:not([data-frame-hidden])')].filter((f) => /reflow\.html/.test(f.src)).every((f) => typeof f.contentWindow.PDFReader === "undefined"),
      textRows: [...document.querySelector('#runtime-host .runtime-frame[data-mareader-slot="active"] iframe.pane-frame[src*="reflow.html"]').contentDocument.querySelectorAll('.tx-stream-col > [data-block-index]')].map((row) => {
        const r = row.getBoundingClientRect();
        return { index: Number(row.dataset.blockIndex), top: r.top, bottom: r.bottom };
      }),
      shell: window.__paneVerificationMarker }));
    if (facts.overflow || facts.entries !== 3 || !facts.engineFreeText || facts.shell !== marker) throw new Error(`${name} failed visual-layout facts: ${JSON.stringify(facts)}`);
    await page.screenshot({ path: `${out}/${name}.png`, fullPage: true });
    report.screenshots.push({ name, width, facts, panes: settled.host.panes.map((p) => ({ id: p.paneId, bounds: p.bounds })) });
  }
  await page.setViewportSize({ width: 1400, height: 900 });
  const beforeClose = await snap();
  const closed = await closeAndWaitBaseline("pane runtime regressions", false, beforeClose.disposalEpoch + afterLift.host.panes.length);
  if (closed.rasterLane?.active !== 0 || closed.rasterLane?.queued !== 0 || closed.rasterLane?.owners !== 0) throw new Error("closed frames retained host raster leases");
  report.closed = { atBaseline: closed.atBaseline, sessionsOpened: closed.engine.sessionsOpened,
    sessionsDestroyed: closed.engine.sessionsDestroyed, rasterLane: closed.rasterLane };
  writeFileSync(`${out}/pane-runtimes.json`, JSON.stringify(report, null, 2));
  console.log("PANE_RUNTIME_VERIFICATION_JSON " + JSON.stringify(report));
  return report;
}
