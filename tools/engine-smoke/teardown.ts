import { PDFReader } from "./harness.js";

export async function run(): Promise<void> {
  // The settled-work sweep: the advisory worker cleanup plus the drop of any
  // zoom mask a superseded render left on a host. The stub hosts carry no
  // masks, so this walks the wiring — a facade member missing from the bundle
  // throws HERE rather than silently skipping in the reader.
  PDFReader.sweep();
  PDFReader.sweepSnapshots();
  PDFReader.unregisterPage("cont-0-cv");
  PDFReader.unregisterPage("cont-1-cv");
  await PDFReader.destroy();
  // Both are idempotent on an empty session: the shelf sweep after a close
  // runs them with nothing registered and no document.
  PDFReader.sweep();
  PDFReader.sweepSnapshots();
  const stats = PDFReader.stats();
  console.log("destroy ok, stats:", JSON.stringify(stats));
  if (stats.pages !== 0 || stats.thumbs !== 0) throw new Error("leak after destroy");

}
