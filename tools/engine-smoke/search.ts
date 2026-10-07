import { R } from "./harness.js";

export async function run(): Promise<void> {
  // The extraction behind the Rust index; a text-less page yields no items.
  const extracted = await R.extractPageText(1);
  if (!extracted.ok) throw new Error("extractPageText failed: " + JSON.stringify(extracted));
  if (extracted.page !== 1) throw new Error("extractPageText echoed the wrong page: " + extracted.page);
  if (!Array.isArray(extracted.items)) throw new Error("extractPageText items missing");
  console.log("extractPageText ok:", extracted.items.length, "items");

  // Query context and match marking are no-ops without a text layer.
  R.setSearchContext("test");
  R.setActiveMatch(1, 0);
  R.clearHighlights();
  console.log("search context wiring ok");

  // Out-of-range extraction is an error envelope, never a throw.
  const bad = await R.extractPageText(99);
  if (bad.ok) throw new Error("extractPageText must fail out of range: " + JSON.stringify(bad));
  console.log("extractPageText range guard ok");
}
