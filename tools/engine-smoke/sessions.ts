// Two sessions in one realm: the engine-level half of the session-ownership
// tests (docs/session-ownership.md). Every document call names its session;
// these scenarios prove that naming is real isolation, not a label:
//
//   * two sessions render, prefetch and thumbnail side by side, and
//     destroying one leaves the other fully usable;
//   * a retired sid is refused everywhere — async calls resolve
//     `no_session`, sync calls are no-ops — and can never be registered
//     again (sids are monotonic);
//   * work in flight when its session dies settles into THAT session's
//     accounting and never lands in a session created afterwards;
//   * an open racing its session's destroy resolves `no_session` and still
//     tears its loading task (and worker) down;
//   * one realm-level appearance change reaches every live session, and the
//     root backdrop paper follows the publishing session only.
//
// The scenario ends with every session it made retired, so the teardown
// baseline after it still proves the realm drains to zero.

import {
  FakeCanvas,
  FakeCtx,
  PDFReader,
  assertClose,
  bind,
  expectedBakePixel,
  fakeComputed,
  fakeDocument,
  getEl,
  isScrubActive,
  newSession,
  setFakeComputed,
  type BoundReader,
} from "./harness.js";

function firstPixel(id: string): number[] {
  const cv = getEl(id) as unknown as { _ctx: FakeCtx };
  return Array.from(cv._ctx.getImageData(0, 0, 1, 1).data).slice(0, 3);
}

function paper(): string {
  const root = getEl("documentElement") as unknown as {
    style: { getPropertyValue: (name: string) => string };
  };
  return root.style.getPropertyValue("--pdf-paper");
}

function themeRoot(
  filter: string,
  blend: string,
  paper: string,
  token: string,
): ReturnType<typeof getEl> & { _themeComputed: { "--canvas-filter": string; "--canvas-blend": string; paper: string }; _style: string } {
  const root = getEl("pane-theme-" + token) as ReturnType<typeof getEl> & {
    _themeComputed: { "--canvas-filter": string; "--canvas-blend": string; paper: string };
    _style: string;
  };
  root._themeComputed = { "--canvas-filter": filter, "--canvas-blend": blend, paper };
  root._style = token;
  const localVars = new Map<string, string>();
  (root as unknown as { style: { setProperty: (name: string, value: string) => void; removeProperty: (name: string) => void; getPropertyValue: (name: string) => string } }).style = {
    setProperty: (name, value) => { localVars.set(name, value); },
    removeProperty: (name) => { localVars.delete(name); },
    getPropertyValue: (name) => localVars.get(name) ?? "",
  };
  (root as unknown as { getAttribute: (name: string) => string | null }).getAttribute =
    (name) => name === "style" ? root._style : null;
  root.appendChild = <T>(child: T): T => {
    (root.children as unknown[]).push(child);
    if (child && typeof child === "object") (child as { parentElement?: unknown }).parentElement = root;
    return child;
  };
  return root;
}

function stubHost(id: string, root?: ReturnType<typeof getEl>): void {
  const host = getEl(id) as unknown as {
    querySelector: () => unknown;
    closest: (selector: string) => ReturnType<typeof getEl> | null;
  };
  host.querySelector = () => ({ classList: { toggle() {} } });
  host.closest = (selector) => selector === "[data-pane-root]" ? (root ?? null) : null;
}

async function openIn(sid: number, path: string): Promise<BoundReader> {
  const opened = await PDFReader.open(sid, path);
  if (!opened.ok) throw new Error(`open in session ${sid} failed: ${JSON.stringify(opened)}`);
  return bind(sid);
}

function assertRefused(name: string, result: { ok: boolean; error?: { name: string } }): void {
  if (result.ok || result.error?.name !== "no_session") {
    throw new Error(`${name} on a retired sid must resolve no_session, got ${JSON.stringify(result)}`);
  }
}

export async function run(): Promise<void> {
  const before = PDFReader.stats();

  // --- Independent sessions ------------------------------------------------
  const sidA = newSession();
  const sidB = newSession();
  const A = await openIn(sidA, "/fake/book.pdf");
  const B = await openIn(sidB, "/fake/blend-book.pdf");
  const live = PDFReader.sessions();
  if (!live.includes(sidA) || !live.includes(sidB)) {
    throw new Error("both sessions must be live, got " + JSON.stringify(live));
  }
  // A second document in a session that holds one is refused: a new
  // document is a new session.
  const again = await PDFReader.open(sidA, "/fake/book.pdf");
  if (again.ok || again.error.name !== "session_in_use") {
    throw new Error("a second open in one session must be refused, got " + JSON.stringify(again));
  }

  // Each session registers its own pages and pins a distinct appearance
  // root. The two documents deliberately request opposite bake pipelines:
  // this catches a realm-global cache that would make both pages use whichever
  // pane wrote its tokens last.
  const rootA = themeRoot("none", "normal", "#ffffff", "a-v1");
  const rootB = themeRoot("invert(1)", "normal", "#000000", "b-v1");
  stubHost("two-a-pg", rootA);
  stubHost("two-b-pg", rootB);
  A.registerPage(1, "two-a-cv", "two-a-pg");
  B.registerPage(2, "two-b-cv", "two-b-pg");
  const [ra, rb] = await Promise.all([
    A.renderPage("two-a-cv", 1.0, false),
    B.renderPage("two-b-cv", 1.0, false),
  ]);
  if (!ra.ok || !rb.ok) throw new Error("side-by-side renders failed: " + JSON.stringify([ra, rb]));
  const statsA = PDFReader.sessionStats(sidA);
  const statsB = PDFReader.sessionStats(sidB);
  if (!statsA || !statsB || statsA.pages !== 1 || statsB.pages !== 1) {
    throw new Error("each session must hold exactly its own page: " + JSON.stringify([statsA?.pages, statsB?.pages]));
  }
  if (statsA.rendersCompleted < 1 || statsB.rendersCompleted < 1) {
    throw new Error("each session must count its own renders");
  }
  await PDFReader.refreshTheme();
  const pixelA = firstPixel("two-a-cv");
  const pixelB = firstPixel("two-b-cv");
  if (pixelA[0] !== 64 || pixelB[0] !== 0) {
    throw new Error("each PDF must bake through its own pane theme: " + JSON.stringify({ pixelA, pixelB }));
  }
  console.log("two sessions ok: distinct pane-root filters bake independently", { pixelA, pixelB });

  // Editing B's pane pipeline must rebake B alone; A's settled raster and
  // generation remain unchanged even though refreshTheme is a realm broadcast.
  const rootBNode = rootB as unknown as { _themeComputed: { "--canvas-filter": string; "--canvas-blend": string; paper: string }; _style: string };
  rootBNode._themeComputed = { "--canvas-filter": "brightness(0.5)", "--canvas-blend": "normal", paper: "#808080" };
  rootBNode._style = "b-v2";
  const rendersABefore = PDFReader.sessionStats(sidA)?.rendersCompleted;
  await PDFReader.refreshTheme();
  // Untouched means untouched: A is not re-rendered either, however the
  // broadcast reached it (a re-render to the same pixels still flashes).
  const rendersAAfter = PDFReader.sessionStats(sidA)?.rendersCompleted;
  if (rendersAAfter !== rendersABefore) {
    throw new Error(`B's pane edit re-rendered A: ${rendersABefore} -> ${rendersAAfter}`);
  }
  const pixelAAfter = firstPixel("two-a-cv");
  const pixelBAfter = firstPixel("two-b-cv");
  if (pixelAAfter[0] !== pixelA[0] || pixelBAfter[0] === pixelB[0]) {
    throw new Error("a pane theme edit must affect only its PDF session: " + JSON.stringify({ pixelAAfter, pixelBAfter }));
  }
  console.log("pane theme update ok: B rebaked while A remained unchanged", { pixelAAfter, pixelBAfter });

  // Two panes showing the same mode carry the SAME page ids. A page
  // registered with its own elements is pinned to them: each session paints
  // its own canvas, and the element a document-wide id lookup would find
  // (the decoy) is never touched.
  const twinA = fakeDocument.createElement("canvas") as unknown as FakeCanvas & { width: number };
  const twinB = fakeDocument.createElement("canvas") as unknown as FakeCanvas & { width: number };
  const decoy = getEl("twin-cv") as unknown as { width: number };
  A.registerPage(1, "twin-cv", "twin-pg", twinA as unknown as HTMLCanvasElement, null);
  B.registerPage(2, "twin-cv", "twin-pg", twinB as unknown as HTMLCanvasElement, null);
  const [ta, tb] = await Promise.all([
    A.renderPage("twin-cv", 1.0, false),
    B.renderPage("twin-cv", 1.0, false),
  ]);
  if (!ta.ok || !tb.ok) throw new Error("pinned twin renders failed: " + JSON.stringify([ta, tb]));
  if (!(twinA.width > 0) || !(twinB.width > 0)) {
    throw new Error(`each session must paint its own pinned canvas (A ${twinA.width}, B ${twinB.width})`);
  }
  if (decoy.width !== 0) throw new Error("a pinned render reached the element the id resolves to in the document");
  A.unregisterPage("twin-cv");
  B.unregisterPage("twin-cv");
  console.log("pinned pages ok: same ids in two sessions, each paints its own element");

  // Prefetch and thumbnails are per session: suspending A's prefetches
  // must not hold B's.
  A.suspendPrefetches();
  await B.prefetchThumb(3, 0.25);
  if (!B.hasThumb(3, 0.25)) throw new Error("B's prefetch must land in B's cache");
  if (A.hasThumb(3, 0.25)) throw new Error("B's prefetch leaked into A's cache");
  A.resumePrefetches();
  await A.prefetchThumb(4, 0.25);
  if (!A.hasThumb(4, 0.25) || B.hasThumb(4, 0.25)) throw new Error("A's prefetch crossed sessions");
  console.log("independent prefetch ok: suspension and caches are per session");

  // --- Root paper follows the publisher -----------------------------------
  A.setPaper("#202020");
  B.setPaper("#f0e0c0");
  const panePaper = (root: ReturnType<typeof getEl>): string =>
    (root as unknown as { style: { getPropertyValue: (name: string) => string } }).style.getPropertyValue("--pane-pdf-paper-baked");
  const localA = panePaper(rootA);
  const localB = panePaper(rootB);
  if (!localA || !localB || localA === localB) {
    throw new Error(`each PDF session must publish its own baked paper: A=${localA} B=${localB}`);
  }
  // B opened last, so B publishes globally while both sessions retain their
  // own pane-local baked-paper values.
  const paperB = paper();
  PDFReader.presentSession(sidA);
  const paperA = paper();
  if (!paperA || !paperB || paperA === paperB) {
    throw new Error(`presenting must swap the root paper: A=${paperA} B=${paperB}`);
  }
  // A non-publisher's paper change leaves the root alone.
  B.setPaper("#e8e0d0");
  if (paper() !== paperA) throw new Error("a non-publishing session repainted the root paper");
  if (panePaper(rootB) === localB) throw new Error("a non-publishing session did not update its own pane paper");
  console.log("paper publisher ok: shared MRU paper and distinct per-session papers", { localA, localB });

  // --- One appearance broadcast, every session ----------------------------
  // Appearance is global; the rasters it is baked into are per session. One
  // refreshTheme must re-bake BOTH sessions' pages, each on its own chain.
  const savedTheme = { ...fakeComputed };
  // From here the fake pane roots inherit the one global theme, matching the
  // product with independent mode off. The earlier assertions deliberately
  // gave them separate pipelines.
  const followsGlobal = new Proxy({}, {
    get: (_target, key: string) => (fakeComputed as unknown as Record<string, string | undefined>)[key],
  }) as typeof fakeComputed;
  (rootA as unknown as { _themeComputed: typeof fakeComputed })._themeComputed = followsGlobal;
  (rootB as unknown as { _themeComputed: typeof fakeComputed })._themeComputed = followsGlobal;
  setFakeComputed({ "--canvas-filter": "none", "--canvas-blend": "multiply", paper: "#ffffff" });
  await PDFReader.refreshTheme();
  const rawA = firstPixel("two-a-cv");
  const rawB = firstPixel("two-b-cv");
  const darkFilter = "invert(0.92) hue-rotate(180deg) saturate(0.85) brightness(1.02)";
  setFakeComputed({ "--canvas-filter": darkFilter, "--canvas-blend": "screen", paper: "#131316" });
  await PDFReader.refreshTheme();
  const darkPaper = [19, 19, 22];
  assertClose(
    new Uint8ClampedArray(firstPixel("two-a-cv")),
    expectedBakePixel(rawA, darkFilter, "screen", darkPaper),
    "session A re-baked by the broadcast",
  );
  assertClose(
    new Uint8ClampedArray(firstPixel("two-b-cv")),
    expectedBakePixel(rawB, darkFilter, "screen", darkPaper),
    "session B re-baked by the broadcast",
  );
  // The scrub window spans every session and the class leaves only once
  // all of them have settled out of it.
  await PDFReader.setScrubMode(true);
  if (!isScrubActive()) throw new Error("scrub must raise the appearance-scrubbing class");
  await PDFReader.setScrubMode(false);
  if (isScrubActive()) throw new Error("the class must leave once every session exits the scrub");
  setFakeComputed(savedTheme);
  await PDFReader.refreshTheme();
  console.log("appearance broadcast ok: both sessions re-baked, one scrub window");

  // --- Stale async: work in flight when its session dies ------------------
  A.registerPage(2, "two-a2-cv", "two-a-pg");
  const inFlight = A.renderPage("two-a2-cv", 1.0, false);
  const pagesB = PDFReader.sessionStats(sidB)!.pages;
  // Independent prefetch across a disposal: both sessions prefetch, A dies
  // with its prefetch in flight, and B's prefetch still lands — in B.
  const prefetchA = A.prefetchThumb(5, 0.25);
  const prefetchB = B.prefetchThumb(5, 0.25);
  await PDFReader.destroySession(sidA);
  // Retiring the publisher hands the backdrop to the most recently
  // presented live session (B) — the split workspace keeps standing on the
  // colour it was last given instead of dropping it.
  if (paper() !== "#e8e0d0") {
    throw new Error("retiring the publisher must fall back to the last presented session's paper, got " + paper());
  }
  const late = await inFlight;
  await Promise.all([prefetchA, prefetchB]);
  if (!B.hasThumb(5, 0.25)) throw new Error("B's prefetch must survive A's disposal");
  // A look change re-bakes the cards a rail can SEE and leaves the rest one
  // generation behind — a prefetched entry nobody ever showed has no canvas to
  // repaint, so the synchronous probe reads a miss BY DESIGN (its raw is what
  // makes that cheap). Disposal is what this checks: B must still HOLD the
  // entry, and asking for the stale card must bring it current from that raw.
  const heldB = PDFReader.sessionStats(sidB)!.thumbs;
  if (heldB < 2) {
    throw new Error("A's disposal must not evict B's thumbnails, B holds " + heldB);
  }
  if (B.hasThumb(3, 0.25)) {
    throw new Error("a look change must not re-bake a cached thumb no rail is showing");
  }
  const rebakeB3 = await B.renderThumb("thumb-3", 3, 0.25);
  if (!rebakeB3.ok) throw new Error("stale thumb must re-bake from raw: " + JSON.stringify(rebakeB3));
  if (!B.hasThumb(3, 0.25)) throw new Error("asking for a stale thumb must leave it current");
  if (A.hasThumb(5, 0.25)) throw new Error("a disposed session must answer no thumbnails");
  if (late.ok) throw new Error("a render whose session died must not report success");
  const sidC = newSession();
  const C = await openIn(sidC, "/fake/book.pdf");
  const statsC = PDFReader.sessionStats(sidC)!;
  if (statsC.pages !== 0 || statsC.rendersCompleted !== 0 || statsC.rendersCancelled !== 0) {
    throw new Error("a dead session's late settle landed in the next session: " + JSON.stringify(statsC));
  }
  if (PDFReader.sessionStats(sidB)!.pages !== pagesB) throw new Error("destroying A touched B's pages");
  console.log("stale async ok: A's late render settled into A, not C or B");

  // B still works after A's teardown.
  const rb2 = await B.renderPage("two-b-cv", 1.2, false);
  if (!rb2.ok) throw new Error("B must keep rendering after A's teardown: " + JSON.stringify(rb2));
  // C's open took the presentation with nothing detected yet — the
  // backdrop HOLDS B's colour rather than flashing to the theme paper.
  if (paper() !== "#e8e0d0") {
    throw new Error("a fresh presentation with no colour must hold the previous paper, got " + paper());
  }
  PDFReader.presentSession(sidB);
  if (paper() !== "#e8e0d0") throw new Error("presenting B must publish its paper, got " + paper());
  console.log("survivor ok: B renders and publishes after A is gone");

  // --- A retired sid is refused, and never reused -------------------------
  assertRefused("open", await PDFReader.open(sidA, "/fake/book.pdf"));
  assertRefused("renderPage", await A.renderPage("two-a-cv", 1.0, false));
  assertRefused("renderThumb", await A.renderThumb("two-thumb-cv", 1, 0.25));
  assertRefused("extractPageText", await A.extractPageText(1));
  A.registerPage(1, "ghost-cv", "two-a-pg"); // a no-op, not a resurrection
  if (A.hasThumb(4, 0.25) || A.takePaperFrame("two-a-cv") !== null) {
    throw new Error("a retired session still answered sync reads");
  }
  if (PDFReader.sessionStats(sidA) !== null) throw new Error("a retired sid still reports stats");
  if (PDFReader.createSession(sidA) || PDFReader.createSession(sidB)) {
    throw new Error("a used sid must never be registered again");
  }
  console.log("retired sid ok: refused everywhere, never re-registered");

  // --- An open racing its session's destroy --------------------------------
  const sidD = newSession();
  const pending = PDFReader.open(sidD, "/fake/book.pdf");
  await PDFReader.destroySession(sidD);
  const raced = await pending;
  assertRefused("open raced by destroy", raced);
  console.log("open/destroy race ok: no_session, task torn down");

  // --- Retire the rest and prove the realm balanced -----------------------
  C.unregisterPage("none"); // harmless on a live session
  B.unregisterPage("two-b-cv");
  await PDFReader.destroySession(sidB);
  await PDFReader.destroySession(sidC);
  await PDFReader.destroySession(sidB); // idempotent
  const after = PDFReader.stats();
  if (after.sessionsLive !== before.sessionsLive) {
    throw new Error(`sessions left live: ${after.sessionsLive} (was ${before.sessionsLive})`);
  }
  // The scenario runs beside the other scenarios' current session, so the
  // gauges must return to exactly what they were before it began.
  const gaugeKeys = ["pages", "thumbs", "thumbTasks", "activeRenders", "pageQueue", "thumbQueue"] as const;
  for (const key of gaugeKeys) {
    if (after[key] !== before[key]) {
      throw new Error(`retired sessions left ${key}=${after[key]} behind (was ${before[key]})`);
    }
  }
  for (const sid of [sidA, sidB, sidC, sidD]) {
    if (PDFReader.sessions().includes(sid)) throw new Error(`session ${sid} is still live`);
  }
  if (after.workersCreated - before.workersCreated !== after.workersTerminated - before.workersTerminated) {
    throw new Error("a loading task outlived its session (workers unbalanced)");
  }
  if (after.sessionsOpened - before.sessionsOpened !== after.sessionsDestroyed - before.sessionsDestroyed) {
    throw new Error("session open/destroy counts unbalanced");
  }
  console.log("two-session teardown ok: realm balanced");
}

// only the changed file was rewritten
