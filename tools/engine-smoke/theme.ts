import {
  FakeCtx,
  PDFReader,
  assertClose,
  created,
  expectedBakePixel,
  fakeComputed,
  setFakeComputed,
  getEl,
  isScrubActive,
} from "./harness.js";

export async function run(): Promise<void> {
  // 3. DARK MODE REGRESSION TEST. The theme is pre-rendered into every
  // raster, so a refresh must bake the new look into the pages on screen.
  const beforeDark = created.length;
  setFakeComputed({
    "--canvas-filter": "invert(0.92) hue-rotate(180deg) saturate(0.85) brightness(1.02)",
    "--canvas-blend": "screen",
    paper: "#131316",
  });
  await PDFReader.refreshTheme();
  const darkExpect = expectedBakePixel([255, 255, 255], fakeComputed["--canvas-filter"], "screen", [19, 19, 22]);
  const cv0 = getEl("cont-0-cv") as unknown as {
    _ctx: FakeCtx;
    classList: { contains: (name: string) => boolean };
  };
  const darkPx = cv0._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(darkPx, darkExpect, "dark refreshTheme bake");
  console.log("refreshTheme (dark) ok: page pixel", Array.from(darkPx).slice(0, 3), "expected", darkExpect);

  // 4. render another page while dark.
  PDFReader.registerPage(2, "cont-1-cv", "cont-1-pg");
  const r2 = await PDFReader.renderPage("cont-1-cv", 1.5, true);
  if (!r2.ok) throw new Error("render2 failed: " + JSON.stringify(r2));
  const darkAllocs = created.length - beforeDark;
  const cv1 = getEl("cont-1-cv") as unknown as {
    _ctx: FakeCtx;
    classList: { contains: (name: string) => boolean };
  };
  const darkPx2 = cv1._ctx.getImageData(0, 0, 1, 1).data;
  if (darkAllocs < 1) throw new Error("dark bake should allocate at least one intermediate canvas, got " + darkAllocs);
  assertClose(darkPx2, darkExpect, "dark render bake");
  console.log("render ok (dark/baked):", r2.width, "x", r2.height, `(${darkAllocs} canvases)`);

  // 5. Scrub mode — the real-time compositing window a slider drag runs in —
  // must expose raw pixels under the live CSS filter, and re-bake them on
  // exit.
  await PDFReader.setScrubMode(true);
  if (!isScrubActive()) {
    throw new Error("entering scrub must raise the appearance-scrubbing class the CSS keys off");
  }
  const scrubPx = cv0._ctx.getImageData(0, 0, 1, 1).data;
  if (scrubPx[0]! < 200 || scrubPx[1]! < 200 || scrubPx[2]! < 200) {
    throw new Error(
      "scrub should show raw page pixels, got [" +
        Array.from(scrubPx).slice(0, 3).join(",") +
        "]",
    );
  }
  console.log("scrub on ok (raw pixels)", Array.from(scrubPx).slice(0, 3));
  await PDFReader.setScrubMode(false);
  if (isScrubActive()) {
    throw new Error("leaving scrub must drop the appearance-scrubbing class");
  }
  const afterScrub = cv0._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(afterScrub, darkExpect, "scrub off rebake");
  console.log("scrub off ok (rebaked)", Array.from(afterScrub).slice(0, 3));

  // 11. DIM check.
  setFakeComputed({
    "--canvas-filter": "brightness(0.8) saturate(0.75) contrast(0.9)",
    "--canvas-blend": "soft-light",
    paper: "#1a1c1f",
  });
  await PDFReader.refreshTheme();
  PDFReader.registerPage(3, "cont-2-cv", "cont-2-pg");
  const r3 = await PDFReader.renderPage("cont-2-cv", 1.5, true);
  if (!r3.ok) throw new Error("render3 failed: " + JSON.stringify(r3));
  const dimExpect = expectedBakePixel([255, 255, 255], fakeComputed["--canvas-filter"], "soft-light", [26, 28, 31]);
  const cv2 = getEl("cont-2-cv") as unknown as {
    _ctx: FakeCtx;
    classList: { contains: (name: string) => boolean };
  };
  const dimPx = cv2._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(dimPx, dimExpect, "dim bake");
  console.log("dim bake ok: page pixel", Array.from(dimPx).slice(0, 3), "expected", dimExpect);

  // 12. A DARK PRESET WITH A TINT.
  setFakeComputed({
    "--canvas-filter": "invert(0.92) hue-rotate(180deg) saturate(0.85) brightness(1.02) sepia(0.193) saturate(1.21) hue-rotate(76deg)",
    "--canvas-blend": "screen",
    paper: "#131316",
  });
  await PDFReader.refreshTheme();
  PDFReader.registerPage(4, "cont-3-cv", "cont-3-pg");
  const r4 = await PDFReader.renderPage("cont-3-cv", 1.5, true);
  if (!r4.ok) throw new Error("render4 failed: " + JSON.stringify(r4));
  const nightExpect = expectedBakePixel([255, 255, 255], fakeComputed["--canvas-filter"], "screen", [19, 19, 22]);
  const cv3 = getEl("cont-3-cv") as unknown as {
    _ctx: FakeCtx;
    classList: { contains: (name: string) => boolean };
  };
  const nightPx = cv3._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(nightPx, nightExpect, "dark+tint bake");
  console.log("dark+tint bake ok: page pixel", Array.from(nightPx).slice(0, 3), "expected", nightExpect);

  // 13. THE PRE-RENDERED BACKDROP PAPER. A baked page already carries the
  // themed paper in its pixels, so the backdrop must not run the filter +
  // blend a second time over the detected colour — the engine publishes the
  // pre-themed paper as --pdf-paper-baked instead. multiply and screen are
  // identity on paper; the mode where a double pass shows is dim's
  // soft-light, so assert it there.
  const rootProp = (name: string): string => {
    const root = getEl("documentElement") as unknown as {
      style: { getPropertyValue: (n: string) => string };
    };
    return root.style.getPropertyValue(name);
  };
  const hexToRgb = (hex: string): [number, number, number] => {
    const m = /^#([0-9a-f]{6})$/i.exec(hex);
    if (!m || !m[1]) throw new Error("expected a #rrggbb baked paper colour, got " + JSON.stringify(hex));
    const v = m[1];
    return [parseInt(v.slice(0, 2), 16), parseInt(v.slice(2, 4), 16), parseInt(v.slice(4, 6), 16)];
  };

  PDFReader.setPaper("#faf4e8");
  setFakeComputed({
    "--canvas-filter": "brightness(0.8) saturate(0.75) contrast(0.9)",
    "--canvas-blend": "soft-light",
    paper: "#1a1c1f",
  });
  await PDFReader.refreshTheme();
  const dimBackdrop = expectedBakePixel(
    [0xfa, 0xf4, 0xe8],
    fakeComputed["--canvas-filter"],
    "soft-light",
    [0x1a, 0x1c, 0x1f],
  );
  assertClose(
    new Uint8ClampedArray(hexToRgb(rootProp("--pdf-paper-baked"))),
    dimBackdrop,
    "baked backdrop paper after a theme change",
  );
  console.log("baked backdrop paper ok (theme change):", rootProp("--pdf-paper-baked"), "expected", dimBackdrop);

  // The detected paper itself moves (the session lerps it along the page
  // ladder); the pre-themed twin must follow on the same write.
  PDFReader.setPaper("#ffffff");
  const dimBackdropWhite = expectedBakePixel(
    [255, 255, 255],
    fakeComputed["--canvas-filter"],
    "soft-light",
    [0x1a, 0x1c, 0x1f],
  );
  assertClose(
    new Uint8ClampedArray(hexToRgb(rootProp("--pdf-paper-baked"))),
    dimBackdropWhite,
    "baked backdrop paper after a paper publish",
  );
  console.log("baked backdrop paper ok (paper publish):", rootProp("--pdf-paper-baked"), "expected", dimBackdropWhite);

  // A blank session paints no backdrop paper.
  PDFReader.setPaper("");
  if (rootProp("--pdf-paper-baked")) {
    throw new Error("a cleared --pdf-paper must clear --pdf-paper-baked, got " + rootProp("--pdf-paper-baked"));
  }
  console.log("baked backdrop paper ok: cleared on a blank session");
}
