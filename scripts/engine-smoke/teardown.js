import { PDFReader } from "./harness.js";
export async function run() {
    // 9. unregister + destroy
    PDFReader.unregisterPage("cont-0-cv");
    PDFReader.unregisterPage("cont-1-cv");
    await PDFReader.destroy();
    const stats = PDFReader.stats();
    console.log("destroy ok, stats:", JSON.stringify(stats));
    if (stats.pages !== 0 || stats.thumbs !== 0)
        throw new Error("leak after destroy");
}
