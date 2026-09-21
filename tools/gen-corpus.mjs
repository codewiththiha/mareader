// Test-corpus generator for the memory baseline (docs/memory-baseline.md).
//
// The baseline is only as good as its inputs are reproducible, so the text
// formats are generated deterministically rather than hand-picked: the same
// command re-creates the same files on every machine, and the Phase 2 gate
// re-measures against exactly what Phase 0 measured.
//
//   big.txt — 30 MB of plain prose paragraphs
//   big.md  — 15 MB of Markdown, a `# Chapter N` heading every ~28 KB
//
// big.pdf is deliberately NOT generated: a hand-assembled PDF exercises none
// of what a real book does (embedded fonts, cmaps, real text runs), and
// pdf.js's memory profile against one would measure the generator, not the
// reader. Bring a 1,000–3,000-page book from the shelf for that row instead.
//
// Usage: node tools/gen-corpus.mjs [outdir]   (default `corpus/`, git-ignored)

import fs from "node:fs";
import path from "node:path";

const TXT_TARGET = 30 * 1024 * 1024;
const MD_TARGET = 15 * 1024 * 1024;

const PARAGRAPH = "The quick brown fox jumps over the lazy dog. ".repeat(20) + "\n\n";

const outdir = process.argv[2] ?? "corpus";
fs.mkdirSync(outdir, { recursive: true });

function generate(fileName, targetBytes, chunk) {
  const parts = [];
  let size = 0;
  for (let i = 0; size < targetBytes; i++) {
    const piece = Buffer.from(chunk(i));
    parts.push(piece);
    size += piece.byteLength;
  }
  const dest = path.join(outdir, fileName);
  fs.writeFileSync(dest, Buffer.concat(parts));
  return size;
}

const txtSize = generate("big.txt", TXT_TARGET, () => PARAGRAPH);
const mdSize = generate("big.md", MD_TARGET, (i) => `# Chapter ${i + 1}\n\n${PARAGRAPH.repeat(30)}`);

const mb = (bytes) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;
console.log(`${path.join(outdir, "big.txt")} — ${mb(txtSize)}`);
console.log(`${path.join(outdir, "big.md")}  — ${mb(mdSize)}`);
