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
} from "./harness.js";

export async function run(): Promise<void> {
  // 6. thumbnails
  const t = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t.ok) throw new Error("thumb failed: " + JSON.stringify(t));
  console.log("thumb ok:", t.width, t.cached);
  const t2 = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t2.ok || t2.cached !== true) throw new Error("thumb cache hit failed");
  console.log("thumb cache hit ok");

  // 6b. Theme change must blit the NEW bake onto the LIVE thumb canvas
  // without a remount / scroll (the user-visible sidebar bug).
  setFakeComputed({
    "--canvas-filter": "invert(0.92) hue-rotate(180deg) saturate(0.85) brightness(1.02)",
    "--canvas-blend": "screen",
    paper: "#131316",
  });
  await PDFReader.refreshTheme();
  const liveThumb = getEl("thumb-1") as unknown as { _ctx: FakeCtx };
  const liveThumbPx = liveThumb._ctx.getImageData(0, 0, 1, 1).data;
  const liveThumbExpect = expectedBakePixel(
    [255, 255, 255],
    fakeComputed["--canvas-filter"],
    "screen",
    [19, 19, 22],
  );
  assertClose(liveThumbPx, liveThumbExpect, "live thumb after refreshTheme");
  console.log("live thumb refreshTheme ok:", Array.from(liveThumbPx).slice(0, 3));

  // 7. theme change marks cached thumbs STALE.
  PDFReader.cancelThumb("thumb-1");
  setFakeComputed({ "--canvas-filter": "brightness(0.8) saturate(0.75) contrast(0.9)", "--canvas-blend": "soft-light" });
  await PDFReader.refreshTheme();
  const t3 = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t3.ok) throw new Error("thumb after theme change failed: " + JSON.stringify(t3));
  const t4 = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t4.ok || t4.cached !== true) throw new Error("rebaked thumb should hit cache, got " + JSON.stringify(t4));
  console.log("lazy thumb re-bake ok");

}
