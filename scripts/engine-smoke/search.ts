import { PDFReader } from "./harness.js";

export async function run(): Promise<void> {
  // 11. Search extraction surface — the data source of the Rust index.
  // The harness pdf has no text items, so an extraction must still resolve
  // `{ok:true, page, items:[]}` (an empty book page is a valid page).
  const extracted = await PDFReader.extractPageText(1);
  if (!extracted.ok) throw new Error("extractPageText failed: " + JSON.stringify(extracted));
  if (extracted.page !== 1) throw new Error("extractPageText echoed the wrong page: " + extracted.page);
  if (!Array.isArray(extracted.items)) throw new Error("extractPageText items missing");
  console.log("extractPageText ok:", extracted.items.length, "items");

  // Publishing a query context and marking/clearing matches must be no-ops
  // on this text-layerless harness (the highlight pass is span-driven).
  PDFReader.setSearchContext("test");
  PDFReader.setActiveMatch(1, 0);
  PDFReader.clearHighlights();
  console.log("search context wiring ok");

  // Out-of-range extraction is an error envelope, never a throw.
  const bad = await PDFReader.extractPageText(99);
  if (bad.ok) throw new Error("extractPageText must fail out of range: " + JSON.stringify(bad));
  console.log("extractPageText range guard ok");
}
