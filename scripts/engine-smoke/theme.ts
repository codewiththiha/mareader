import {
  EngineResult,
  FakeCtx,
  PDFReader,
  RenderPayload,
  assertClose,
  created,
  expectedBakePixel,
  fakeComputed,
  setFakeComputed,
  getEl,
  isLivePipelineActive,
} from "./harness.js";

function assertRawWhite(data: Uint8ClampedArray, label: string): void {
  if (data[0] !== 255 || data[1] !== 255 || data[2] !== 255) {
    throw new Error(`${label} should retain raw white pixels, got ${Array.from(data).slice(0, 3)}`);
  }
}

export async function run(): Promise<void> {
  const livePipeline = isLivePipelineActive();
  // 3. DARK MODE REGRESSION TEST.
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
  if (livePipeline) {
    assertRawWhite(darkPx, "live refreshTheme");
    if (!cv0.classList.contains("canvas-raw")) {
      throw new Error("live refreshTheme should keep the page tagged canvas-raw");
    }
    console.log("refreshTheme (live) ok: raw page remains under CSS pipeline");
  } else {
    assertClose(darkPx, darkExpect, "dark refreshTheme bake");
    console.log("refreshTheme (dark) ok: page pixel", Array.from(darkPx).slice(0, 3), "expected", darkExpect);
  }

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
  if (livePipeline) {
    if (darkAllocs !== 0) {
      throw new Error("live dark render should not allocate bake canvases, got " + darkAllocs);
    }
    assertRawWhite(darkPx2, "live dark render");
    if (!cv1.classList.contains("canvas-raw")) {
      throw new Error("live render should tag its raw page canvas");
    }
    console.log("render ok (dark/live):", r2.width, "x", r2.height, "(no bake canvases)");
  } else {
    if (darkAllocs < 1) throw new Error("dark bake should allocate at least one intermediate canvas, got " + darkAllocs);
    assertClose(darkPx2, darkExpect, "dark render bake");
    console.log("render ok (dark/baked):", r2.width, "x", r2.height, `(${darkAllocs} canvases)`);
  }

  // 5. Scrub mode must expose raw pixels. In live mode the exposure is
  // already permanent, so both API calls are intentional no-ops and the raw
  // marker remains in place. Baked mode still verifies the enter/leave swap.
  await PDFReader.setScrubMode(true);
  const scrubPx = cv0._ctx.getImageData(0, 0, 1, 1).data;
  if (livePipeline) {
    assertRawWhite(scrubPx, "live scrub");
    if (!cv0.classList.contains("canvas-raw")) {
      throw new Error("live scrub should keep the page tagged canvas-raw");
    }
    console.log("scrub on ok (live pipeline already active)");
  } else {
    if (scrubPx[0]! < 200 || scrubPx[1]! < 200 || scrubPx[2]! < 200) {
      throw new Error(
        "scrub should show raw page pixels, got [" +
          Array.from(scrubPx).slice(0, 3).join(",") +
          "]",
      );
    }
    console.log("scrub on ok (raw pixels)", Array.from(scrubPx).slice(0, 3));
  }
  await PDFReader.setScrubMode(false);
  const afterScrub = cv0._ctx.getImageData(0, 0, 1, 1).data;
  if (livePipeline) {
    assertRawWhite(afterScrub, "live scrub off");
    if (!cv0.classList.contains("canvas-raw")) {
      throw new Error("live scrub off should not remove the canvas-raw marker");
    }
    console.log("scrub off ok (live pipeline stays active)");
  } else {
    assertClose(afterScrub, darkExpect, "scrub off rebake");
    console.log("scrub off ok (rebaked)", Array.from(afterScrub).slice(0, 3));
  }

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
  if (livePipeline) {
    assertRawWhite(dimPx, "live dim render");
    if (!cv2.classList.contains("canvas-raw")) {
      throw new Error("live dim render should tag its raw page canvas");
    }
    console.log("dim render ok (live CSS pipeline)");
  } else {
    assertClose(dimPx, dimExpect, "dim bake");
    console.log("dim bake ok: page pixel", Array.from(dimPx).slice(0, 3), "expected", dimExpect);
  }

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
  if (livePipeline) {
    assertRawWhite(nightPx, "live dark+tint render");
    if (!cv3.classList.contains("canvas-raw")) {
      throw new Error("live dark+tint render should tag its raw page canvas");
    }
    console.log("dark+tint render ok (live CSS pipeline)");
  } else {
    assertClose(nightPx, nightExpect, "dark+tint bake");
    console.log("dark+tint bake ok: page pixel", Array.from(nightPx).slice(0, 3), "expected", nightExpect);
  }

  // 13. THE PIPELINE SWITCH (Appearance > Rendering). Flipping to baked must
  // burn the CURRENT pipeline into the rasters already on screen and drop the
  // raw markers; flipping back must expose the raws again untouched. The
  // pipeline is left as it started so the later scenarios still describe the
  // engine's default mode.
  const startedLive = PDFReader.isLivePipeline();
  await PDFReader.setLivePipeline(false);
  if (PDFReader.isLivePipeline()) throw new Error("engine should report baked mode after the switch");
  const bakedPx = cv3._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(bakedPx, nightExpect, "switch to baked");
  if (cv3.classList.contains("canvas-raw")) {
    throw new Error("a baked page must not keep the canvas-raw marker");
  }
  console.log("pipeline switch ok (baked):", Array.from(bakedPx).slice(0, 3));

  await PDFReader.setLivePipeline(true);
  if (!PDFReader.isLivePipeline()) throw new Error("engine should report live mode after the switch back");
  const backLivePx = cv3._ctx.getImageData(0, 0, 1, 1).data;
  assertRawWhite(backLivePx, "switch back to live");
  if (!cv3.classList.contains("canvas-raw")) {
    throw new Error("a live page must carry the canvas-raw marker again");
  }
  console.log("pipeline switch ok (live): raw pixels restored");

  if (!startedLive) await PDFReader.setLivePipeline(false);
}
