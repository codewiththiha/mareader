import { PDFReader, created, getEl, setFakeComputed, trackCreatedCanvases, } from "./harness.js";
export async function run() {
    // 2. register + render a page (identity pipeline first)
    PDFReader.registerPage({ canvasId: "cont-0-cv", hostId: "cont-0-pg", page: 1 });
    const page0 = getEl("cont-0-pg");
    page0.querySelector = () => ({ classList: { toggle() { } } });
    const r1 = await PDFReader.renderPage("cont-0-cv", 1.5, true);
    if (!r1.ok)
        throw new Error("render failed: " + JSON.stringify(r1));
    console.log("render ok (identity):", r1.width, "x", r1.height);
    // 2b. light theme with multiply over PURE WHITE = identity pipeline: a
    // render must allocate ZERO page-sized bake canvases (the default-theme
    // fast path). Small canvases (the 1x1 paper sampler) don't count.
    trackCreatedCanvases();
    setFakeComputed({ "--canvas-filter": "none", "--canvas-blend": "multiply" });
    PDFReader.registerPage({ canvasId: "cont-0-cv", hostId: "cont-0-pg", page: 1 });
    await PDFReader.renderPage("cont-0-cv", 1.5, true);
    const bakeCanvases = created.filter((el) => el.tagName === "CANVAS" && el.width > 10).length;
    if (bakeCanvases !== 0)
        throw new Error("identity pipeline allocated bake canvases: " + bakeCanvases);
    console.log("identity fast path ok (0 bake canvases)");
    // 8. burst coalescing
    const p1 = PDFReader.renderPage("cont-0-cv", 1.0, true);
    const p2 = PDFReader.renderPage("cont-0-cv", 1.2, true);
    const p3 = PDFReader.renderPage("cont-0-cv", 1.4, true);
    const [a, b, c] = await Promise.all([p1, p2, p3]);
    const fmt = (r) => r.ok ? "ok" : r.error.name;
    console.log("burst coalesce:", fmt(a), fmt(b), fmt(c));
}
