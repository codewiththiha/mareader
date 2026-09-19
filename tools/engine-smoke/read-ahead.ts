import assert from "node:assert/strict";
import { FakeCanvas, PDFReader, emitDocumentScroll, fakePdf, getEl } from "./harness.js";

export async function run(): Promise<void> {
  const getPage = fakePdf.getPage;
  const restores: Array<() => void> = [];
  const starts: Array<{ page: number; phase: string; offset: number }> = [];
  const textPages = new Set<number>();
  const linkPages = new Set<number>();
  const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
  let anchor = 2;
  const scroller = {
    scrollTop: 800, scrollLeft: 0, clientHeight: 800, clientWidth: 600,
    scrollHeight: 8000, scrollWidth: 600,
    getAttribute: (name: string) => name === "data-raster-axis" ? "vertical" : name === "data-raster-anchor" ? String(anchor) : null,
    getBoundingClientRect: () => ({ top: 0, bottom: 800, left: 0, right: 600, width: 600, height: 800 }),
    contains: (element: { id?: string }) => element.id?.startsWith("cont-") ?? false,
  };
  try {
    fakePdf.getPage = async (page: number) => {
      const proxy = await getPage(page);
      return {
        ...proxy,
        render: (options: Parameters<typeof proxy.render>[0]) => {
          starts.push({ page, phase: PDFReader.stats().raster.phase, offset: scroller.scrollTop });
          return proxy.render(options);
        },
        getTextContent: async () => { textPages.add(page); return proxy.getTextContent(); },
        getAnnotations: async () => { linkPages.add(page); return proxy.getAnnotations(); },
      };
    };
    for (let page = 1; page <= 5; page++) {
      const id = `cont-${page - 1}`;
      const host = getEl(`${id}-pg`) as ReturnType<typeof getEl> & { closest?: () => unknown };
      const rect = host.getBoundingClientRect;
      const query = host.querySelector;
      const closest = host.closest;
      host.closest = () => scroller;
      host.getBoundingClientRect = () => {
        const top = (page - 1) * 800 - scroller.scrollTop;
        return { x: 0, y: top, left: 0, right: 600, top, bottom: top + 800, width: 600, height: 800 };
      };
      host.querySelector = (() => new FakeCanvas("div")) as unknown as typeof host.querySelector;
      restores.push(() => {
        host.getBoundingClientRect = rect;
        host.querySelector = query;
        if (closest) host.closest = closest;
        else delete host.closest;
      });
      if (page <= 4) PDFReader.registerPage(page, `${id}-cv`, `${id}-pg`);
    }

    // Deliberately request in the wrong order: page 2 must win and its next
    // two pages must already have pixels/text/links before entering the view.
    const initial = await Promise.all([4, 1, 3, 2].map((page) => PDFReader.renderPage(`cont-${page - 1}-cv`, 1, true)));
    assert.ok(initial.every((result) => result.ok));
    assert.equal(starts[0]?.page, 2);
    for (const page of [1, 2, 3, 4]) {
      assert.ok(getEl(`cont-${page - 1}-cv`).width > 0, `page ${page} should be warm`);
      assert.ok(textPages.has(page), `page ${page} should have selectable text`);
      assert.ok(linkPages.has(page), `page ${page} should have its links`);
    }

    // Still actively tracking, halfway between 2 and 3: page 5 is wholly
    // offscreen (1.5 screens away), but is already part of the warm ring.
    anchor = 3;
    scroller.scrollTop = 1200;
    emitDocumentScroll(scroller, 1000);
    assert.equal(PDFReader.stats().raster.phase, "Tracking");
    PDFReader.registerPage(5, "cont-4-cv", "cont-4-pg");
    const ahead = await PDFReader.renderPage("cont-4-cv", 1, true);
    assert.ok(ahead.ok);
    const five = starts.find((start) => start.page === 5);
    assert.equal(five?.phase, "Tracking", "normal scrolling must not wait for settling to render the next page");
    assert.ok(five && five.offset + scroller.clientHeight < 3200, "page 5 must finish offscreen");
    assert.ok(textPages.has(5));

    // A real jump remains a fling: newly requested work cannot allocate until
    // settling, even though it is part of the destination's warm window.
    PDFReader.unregisterPage("cont-4-cv");
    anchor = 5;
    scroller.scrollTop = 4000;
    emitDocumentScroll(scroller, 1016);
    assert.equal(PDFReader.stats().raster.phase, "Fling");
    PDFReader.registerPage(5, "cont-4-cv", "cont-4-pg");
    const before = starts.length;
    const landing = PDFReader.renderPage("cont-4-cv", 1, false);
    for (let turn = 0; turn < 3; turn++) await tick();
    assert.equal(starts.length, before);
    assert.ok(PDFReader.stats().raster.queued > 0);
    assert.ok((await landing).ok);
    assert.notEqual(starts.at(-1)?.phase, "Fling");
    assert.equal(PDFReader.stats().raster.fullRendersStartedDuringFling, 0);
    // Horizontal cold start has not dispatched a horizontal scroll yet.
    // Its published scroller bounds/anchor must still put page 2 first.
    const horizontal = {
      ...scroller,
      getAttribute: (name: string) => name === "data-raster-axis" ? "horizontal" : name === "data-raster-anchor" ? "2" : null,
    };
    for (let page = 1; page <= 4; page++) {
      const host = getEl(`hp-${page}-pg`) as ReturnType<typeof getEl> & { closest?: () => unknown };
      const rect = host.getBoundingClientRect;
      host.closest = () => horizontal;
      host.getBoundingClientRect = () => {
        const left = (page - 2) * 600;
        return { x: left, y: 0, left, right: left + 600, top: 0, bottom: 800, width: 600, height: 800 };
      };
      restores.push(() => { host.getBoundingClientRect = rect; delete host.closest; });
      PDFReader.registerPage(page, `hp-${page}-cv`, `hp-${page}-pg`);
    }
    const horizontalStart = starts.length;
    assert.ok((await Promise.all([4, 1, 3, 2].map((page) => PDFReader.renderPage(`hp-${page}-cv`, 1, false)))).every((result) => result.ok));
    assert.equal(starts[horizontalStart]?.page, 2);
    console.log("read-ahead: current/neighbor pixels, offscreen text/links, tracking prepaint, fling suppression and horizontal cold start passed");
  } finally {
    fakePdf.getPage = getPage;
    for (const restore of restores) restore();
    for (let page = 1; page <= 4; page++) PDFReader.unregisterPage(`hp-${page}-cv`);
    for (let page = 2; page <= 5; page++) PDFReader.unregisterPage(`cont-${page - 1}-cv`);
  }
}
