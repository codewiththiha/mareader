// Generate the Phase 0 baseline corpus: two reflowable files sized to
// dominate the wasm half of the webview. Compiled by tsconfig.tools.json to
// `scripts/` like the other tools in this directory, run once with
// `node scripts/gen-corpus.js`.
//
// The sizes are held as constants so a re-run is the re-run that produced
// the committed baseline:
//   big.txt  30 MB of plain paragraphs — reflow blocks/heights/cuts, so the
//            wasm arena grows and the JS half stays quiet.
//   big.md   15 MB of headed chapters — the same reflow cost with the md
//            parser and heading structure in front of it.
//   big.pdf  NOT generated here — no pdf writer ships in devDependencies, and
//            a synthetic single-page PDF would not exercise the pdf.js paths
//            the measurements exist for. The run protocol uses any real book
//            in the 1,000–3,000 page range and records its page count beside
//            the row.

import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { root } from "./repo.js";

const outDir = join(root, "corpus");
mkdirSync(outDir, { recursive: true });
const mb = (bytes: number) => `${(bytes / (1024 * 1024)).toFixed(1)} MB`;

const para = "The quick brown fox jumps over the lazy dog. ".repeat(20) + "\n\n";

// TXT: one paragraph repeated until the 30 MB mark. The paragraphs are
// identical on purpose — the layout carries them all, but nothing in the
// content varies the per-block cost, so the wasm growth reads as block count
// and nothing else.
let txt = "";
while (Buffer.byteLength(txt, "utf8") < 30 * 1024 * 1024) txt += para;
writeFileSync(join(outDir, "big.txt"), txt);
console.log(`big.txt  ${mb(Buffer.byteLength(txt, "utf8"))}`);

// MD: a chapter heading every thirty paragraphs, so the markdown parser has
// structure to walk, until the 15 MB mark.
let md = "";
let chapter = 1;
while (Buffer.byteLength(md, "utf8") < 15 * 1024 * 1024) {
  md += `# Chapter ${chapter}\n\n${para.repeat(30)}`;
  chapter += 1;
}
writeFileSync(join(outDir, "big.md"), md);
console.log(`big.md   ${mb(Buffer.byteLength(md, "utf8"))}`);
