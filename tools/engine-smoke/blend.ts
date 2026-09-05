import {
  PDFReader,
  fakeLocalStorage,
  getEl,
  setFakePageColors,
} from "./harness.js";

/** The --pdf-paper the engine currently has published. */
function paper(): string {
  const root = getEl("documentElement") as unknown as {
    style: { getPropertyValue: (name: string) => string };
  };
  return root.style.getPropertyValue("--pdf-paper");
}

/** The first pixel of a frame's data, as an rgb triple. */
function firstPixel(data: Uint8ClampedArray | undefined): [number, number, number] {
  return [data?.[0] ?? -1, data?.[1] ?? -1, data?.[2] ?? -1];
}

/** `a` within ±1 of `b` per channel (a downscaled uniform raster is exact,
 * but the assertion stays tolerant to rounding by a single step). */
function isColour(actual: [number, number, number], want: [number, number, number]): boolean {
  return actual.every((v, i) => Math.abs(v - want[i]!) <= 1);
}

export async function run(): Promise<void> {
  // The colour DECISIONS (detection, the palette, the scroll interpolation)
  // live in the pdf-paper crate behind the Rust paper session and are
  // covered by cargo tests. This scenario walks the ENGINE side of the
  // contract: the frames it hands over and the --pdf-paper it paints when
  // told to.
  setFakePageColors({ 1: "#404040", 2: "#ffffff", 3: "#a0a0a0", 4: "#ffffff", 5: "#ffffff" });

  // --- a live render parks its raw frame for the session to drain ----------
  const opened = await PDFReader.open("/fake/blend-book.pdf");
  if (!opened.ok) throw new Error("open failed: " + JSON.stringify(opened));
  PDFReader.registerPage(1, "blend-1-cv", "blend-1-pg");
  const rendered = await PDFReader.renderPage("blend-1-cv", 1.0, true);
  if (!rendered.ok) throw new Error("render failed: " + JSON.stringify(rendered));

  const frame = PDFReader.takePaperFrame("blend-1-cv");
  if (!frame || !frame.data) throw new Error("a live render must stash a paper frame");
  if (frame.page !== 1) throw new Error("stashed frame should be page 1, got " + frame.page);
  if (frame.width < 16 || frame.height < 16) {
    throw new Error("stashed frame should be a real downscale, got " + frame.width + "x" + frame.height);
  }
  if (!isColour(firstPixel(frame.data), [0x40, 0x40, 0x40])) {
    throw new Error("stashed frame carries the RAW page colour, got " + firstPixel(frame.data));
  }
  // The stash drains: a second take has nothing to give.
  if (PDFReader.takePaperFrame("blend-1-cv") !== null) {
    throw new Error("takePaperFrame must drain the stash");
  }
  console.log("paper frame stash ok: page 1's raw pixels handed over + drained");

  // --- setPaper publishes; nothing is written to storage --------------------
  PDFReader.setPaper("#404040");
  if (paper() !== "#404040") {
    throw new Error("setPaper should publish --pdf-paper, got " + paper());
  }
  PDFReader.setPaper("#faf4e8");
  if (paper() !== "#faf4e8") {
    throw new Error("a second setPaper should repaint, got " + paper());
  }
  PDFReader.setPaper("");
  if (paper()) {
    throw new Error("an empty setPaper should clear --pdf-paper, got " + paper());
  }
  if (fakeLocalStorage.size !== 0) {
    throw new Error("the paper pipeline must not touch storage, found " + [...fakeLocalStorage.keys()].join(","));
  }
  console.log("paper publish ok: --pdf-paper set, repainted, cleared; storage untouched");

  // --- offscreen samples carry the page's own paint -------------------------
  const sample2 = await PDFReader.samplePaperPage(2);
  if (!sample2.ok || !sample2.data || sample2.page !== 2) {
    throw new Error("samplePaperPage(2) should resolve a frame, got " + JSON.stringify(sample2));
  }
  if (!isColour(firstPixel(sample2.data), [0xff, 0xff, 0xff])) {
    throw new Error("page 2's sample should be white, got " + firstPixel(sample2.data));
  }
  const sample3 = await PDFReader.samplePaperPage(3);
  if (!sample3.ok || !sample3.data || !isColour(firstPixel(sample3.data), [0xa0, 0xa0, 0xa0])) {
    throw new Error("page 3's sample should be #a0a0a0, got " + firstPixel(sample3.data));
  }
  // A page that cannot answer (past the end) resolves {ok:true} with no
  // frame — a skip for the look-ahead, not an error.
  const none = await PDFReader.samplePaperPage(99);
  if (!none.ok || (none as { data?: Uint8ClampedArray }).data) {
    throw new Error("an out-of-range page must resolve a frameless ok, got " + JSON.stringify(none));
  }
  console.log("paper samples ok: offscreen pages 2 + 3 + a frameless skip past the end");

  // --- a new document drops the previous book's undrained frames -----------
  PDFReader.registerPage(2, "blend-2-cv", "blend-2-pg");
  const r2 = await PDFReader.renderPage("blend-2-cv", 1.0, true);
  if (!r2.ok) throw new Error("render page 2 failed: " + JSON.stringify(r2));
  const f2 = PDFReader.takePaperFrame("blend-2-cv");
  if (!f2 || f2.page !== 2 || !isColour(firstPixel(f2.data), [0xff, 0xff, 0xff])) {
    throw new Error("page 2's live frame should be white, got " + JSON.stringify(f2));
  }
  // Re-stash (a re-render at a new scale), then reopen: the stash must go.
  const r2b = await PDFReader.renderPage("blend-2-cv", 1.5, true);
  if (!r2b.ok) throw new Error("re-render page 2 failed: " + JSON.stringify(r2b));
  if (PDFReader.takePaperFrame("blend-2-cv") === null) {
    throw new Error("the re-render should have re-stashed a frame");
  }
  const reopened = await PDFReader.open("/fake/blend-book.pdf");
  if (!reopened.ok) throw new Error("reopen failed: " + JSON.stringify(reopened));
  if (PDFReader.takePaperFrame("blend-2-cv") !== null) {
    throw new Error("opening a document must drop the previous book's stash");
  }
  console.log("paper stash lifecycle ok: re-render re-stashes, reopen clears");

  // --- the stash is gated on the blend switch -------------------------------
  // While the session says blend is off, a live render pays nothing on the
  // paper pipeline: no downscale, no readback, no stash.
  PDFReader.setPaperActive(false);
  PDFReader.registerPage(3, "blend-3-cv", "blend-3-pg");
  const r3 = await PDFReader.renderPage("blend-3-cv", 1.0, true);
  if (!r3.ok) throw new Error("render page 3 failed: " + JSON.stringify(r3));
  if (PDFReader.takePaperFrame("blend-3-cv") !== null) {
    throw new Error("blend off must skip the paper stash entirely");
  }
  // Back on: the very next render stashes again.
  PDFReader.setPaperActive(true);
  const r3b = await PDFReader.renderPage("blend-3-cv", 1.0, true);
  if (!r3b.ok) throw new Error("re-render page 3 failed: " + JSON.stringify(r3b));
  const f3 = PDFReader.takePaperFrame("blend-3-cv");
  if (!f3 || f3.page !== 3) {
    throw new Error("blend on must restore the stash, got " + JSON.stringify(f3));
  }
  console.log("paper gate ok: blend off skips the stash, blend on restores it");

  // --- a stale per-document cache from an older build is swept on load ------
  // The engine used to remember one colour per book in localStorage; the
  // facade clears that key when it installs, so a reader upgrading from an
  // older build does not carry the dead entries around forever.
  fakeLocalStorage.set("pdfreader.blend-paper.v2", '{"/fake/x.pdf":{"fixed":"#101010"}}');
  PDFReader.clearLegacyPaperCache();
  if (fakeLocalStorage.has("pdfreader.blend-paper.v2")) {
    throw new Error("the legacy paper cache should be swept");
  }
  console.log("paper cache sweep ok: the retired localStorage key is removed");
}
