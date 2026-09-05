import {
  EngineResult,
  FakeCtx,
  PDFReader,
  ThumbPayload,
  assertClose,
  expectedBakePixel,
  fakeComputed,
  setFakeComputed,
  getEl,
  isLivePipelineActive,
} from "./harness.js";

export async function run(): Promise<void> {
  const livePipeline = isLivePipelineActive();
  // 6. thumbnails
  const t = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t.ok) throw new Error("thumb failed: " + JSON.stringify(t));
  console.log("thumb ok:", t.width, t.height);
  // A cache hit is asked about the way the app asks about it: the synchronous
  // probe a cell reads while it is still being built. The render promise used
  // to carry a `cached` flag instead, which arrived after the cell's first
  // frame was composited and so could never do the job its doc claimed.
  if (!PDFReader.hasThumb(1, 0.25)) throw new Error("thumb not cached after render");
  const t2 = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t2.ok) throw new Error("thumb cache hit failed: " + JSON.stringify(t2));
  if (!PDFReader.hasThumb(1, 0.25)) throw new Error("thumb cache lost after hit");
  console.log("thumb cache hit ok");

  // 6b. Theme change must blit the NEW bake onto the LIVE thumb canvas
  // without a remount / scroll (the user-visible sidebar bug).
  setFakeComputed({
    "--canvas-filter": "invert(0.92) hue-rotate(180deg) saturate(0.85) brightness(1.02)",
    "--canvas-blend": "screen",
    paper: "#131316",
  });
  await PDFReader.refreshTheme();
  const liveThumb = getEl("thumb-1") as unknown as {
    _ctx: FakeCtx;
    classList: { contains: (name: string) => boolean };
  };
  const liveThumbPx = liveThumb._ctx.getImageData(0, 0, 1, 1).data;
  const liveThumbExpect = expectedBakePixel(
    [255, 255, 255],
    fakeComputed["--canvas-filter"],
    "screen",
    [19, 19, 22],
  );
  if (livePipeline) {
    if (liveThumbPx[0] !== 255 || liveThumbPx[1] !== 255 || liveThumbPx[2] !== 255) {
      throw new Error("live thumb should retain raw pixels, got " + Array.from(liveThumbPx).slice(0, 3));
    }
    if (!liveThumb.classList.contains("thumb-raw")) {
      throw new Error("live thumb should retain the thumb-raw marker");
    }
    console.log("live thumb refreshTheme ok: raw pixels remain under CSS pipeline");
  } else {
    assertClose(liveThumbPx, liveThumbExpect, "live thumb after refreshTheme");
    console.log("live thumb refreshTheme ok:", Array.from(liveThumbPx).slice(0, 3));
  }

  // 7. theme change marks cached thumbs STALE.
  PDFReader.cancelThumb("thumb-1");
  setFakeComputed({ "--canvas-filter": "brightness(0.8) saturate(0.75) contrast(0.9)", "--canvas-blend": "soft-light" });
  await PDFReader.refreshTheme();
  const t3 = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t3.ok) throw new Error("thumb after theme change failed: " + JSON.stringify(t3));
  // The theme change staled the cached entry; the re-render above must have
  // refreshed it, and the next one must hit.
  if (!PDFReader.hasThumb(1, 0.25)) throw new Error("thumb cache not refreshed after theme change");
  const t4 = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t4.ok) throw new Error("thumb cache hit after theme change failed, got " + JSON.stringify(t4));
  if (!PDFReader.hasThumb(1, 0.25)) throw new Error("thumb cache lost after theme change");
  if (livePipeline) {
    console.log("live thumb cache refresh ok: theme change kept the raw cache path");
  } else {
    console.log("lazy thumb re-bake ok");
  }

}
