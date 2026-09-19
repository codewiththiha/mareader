import assert from "node:assert/strict";
import { PDFReader, fakePdf, fakeDocument, created, trackCreatedCanvases } from "./harness.js";

export async function run(): Promise<void> {
  const originalGetPage = fakePdf.getPage;
  const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
  const drained = async () => { for (let i = 0; i < 5; i++) await tick(); };
  try {
    // An asynchronous getPage may finish after a same-id unmount/remount.
    // The old request must clean up, never paint into the new component.
    let release!: () => void;
    let entered!: () => void;
    const waiting = new Promise<void>((resolve) => { entered = resolve; });
    const gate = new Promise<void>((resolve) => { release = resolve; });
    let cleanup = 0;
    fakePdf.getPage = async (page: number) => {
      const proxy = await originalGetPage(page);
      entered();
      await gate;
      return { ...proxy, cleanup: async () => { cleanup++; } };
    };
    const stale = PDFReader.renderPage("cont-0-cv", 1.5, false);
    await waiting;
    PDFReader.unregisterPage("cont-0-cv");
    PDFReader.registerPage(1, "cont-0-cv", "cont-0-pg");
    fakePdf.getPage = originalGetPage;
    const fresh = PDFReader.renderPage("cont-0-cv", 1.5, false);
    release();
    assert.equal((await stale).ok, false);
    assert.equal((await fresh).ok, true);
    await drained();
    assert.equal(cleanup, 1);
    assert.equal(PDFReader.stats().raster.reservedBytes, 0);

    // Text extraction failure is a successful raster, not a leaked page.
    fakePdf.getPage = async (page: number) => {
      const proxy = await originalGetPage(page);
      return { ...proxy, getTextContent: async () => { throw new Error("text unavailable"); }, cleanup: async () => { cleanup++; } };
    };
    assert.equal((await PDFReader.renderPage("cont-0-cv", 1.6, true)).ok, true);
    await drained();
    assert.equal(cleanup, 2);

    // A synchronous pdf.js render throw still releases the target and page.
    trackCreatedCanvases();
    created.length = 0;
    fakePdf.getPage = async (page: number) => {
      const proxy = await originalGetPage(page);
      return { ...proxy, render: () => { throw new Error("render failed"); }, cleanup: async () => { cleanup++; } };
    };
    assert.equal((await PDFReader.renderPage("cont-0-cv", 1.7, false)).ok, false);
    await drained();
    assert.equal(cleanup, 3);
    assert.ok(created.filter((canvas) => canvas.tagName === "CANVAS").every((canvas) => canvas.width === 0));
    assert.equal(PDFReader.stats().raster.reservedBytes, 0);

    // Context failure exercises the early return before a render task exists.
    const create = fakeDocument.createElement;
    fakePdf.getPage = async (page: number) => ({ ...await originalGetPage(page), cleanup: async () => { cleanup++; } });
    try {
      fakeDocument.createElement = (tag: string) => {
        const element = create(tag);
        if (tag === "canvas") element.getContext = (() => null) as unknown as typeof element.getContext;
        return element;
      };
      assert.equal((await PDFReader.renderPage("cont-0-cv", 1.8, false)).ok, false);
    } finally { fakeDocument.createElement = create; }
    await drained();
    assert.equal(cleanup, 4);
    assert.equal(PDFReader.stats().raster.reservedBytes, 0);
    console.log("raster lifecycle: stale remount, text failure, render throw and context cleanup passed");
  } finally { fakePdf.getPage = originalGetPage; }
}
