import { FakeCtx, PDFReader, assertClose, expectedBakePixel, fakeComputed, setFakeComputed, getEl, R } from "./harness.js";

export async function run(): Promise<void> {
  // Thumbnails.
  const t = await R.renderThumb("thumb-1", 1, 0.25);
  if (!t.ok) throw new Error("thumb failed: " + JSON.stringify(t));
  console.log("thumb ok:", t.width, t.height);
  // A hit is probed the way the app asks: the cell is being built.
  if (!R.hasThumb(1, 0.25)) throw new Error("thumb not cached after render");
  const t2 = await R.renderThumb("thumb-1", 1, 0.25);
  if (!t2.ok) throw new Error("thumb cache hit failed: " + JSON.stringify(t2));
  if (!R.hasThumb(1, 0.25)) throw new Error("thumb cache lost after hit");
  console.log("thumb cache hit ok");

  // A theme change blits the new bake onto the live thumb, no remount needed.
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
  assertClose(liveThumbPx, liveThumbExpect, "live thumb after refreshTheme");
  console.log("live thumb refreshTheme ok:", Array.from(liveThumbPx).slice(0, 3));

  // A theme change marks cached thumbs STALE.
  R.cancelThumb("thumb-1");
  setFakeComputed({ "--canvas-filter": "brightness(0.8) saturate(0.75) contrast(0.9)", "--canvas-blend": "soft-light" });
  await PDFReader.refreshTheme();
  const t3 = await R.renderThumb("thumb-1", 1, 0.25);
  if (!t3.ok) throw new Error("thumb after theme change failed: " + JSON.stringify(t3));
  // The change staled the entry; the re-render refreshed it, so the next hits.
  if (!R.hasThumb(1, 0.25)) throw new Error("thumb cache not refreshed after theme change");
  const t4 = await R.renderThumb("thumb-1", 1, 0.25);
  if (!t4.ok) throw new Error("thumb cache hit after theme change failed, got " + JSON.stringify(t4));
  if (!R.hasThumb(1, 0.25)) throw new Error("thumb cache lost after theme change");
  console.log("lazy thumb re-bake ok");
}
