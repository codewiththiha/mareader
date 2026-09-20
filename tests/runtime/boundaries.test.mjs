import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
const read = (path) => readFileSync(path, "utf8");

test("host loads only workspace WASM and lifecycle JS", () => {
  const html = read("index.html");
  assert.match(html, /apps\/workspace-wasm\/Cargo.toml/);
  const scripts = [...html.matchAll(/<script[^>]+src="([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(scripts, ["/host.js"]);
  assert.doesNotMatch(read("host/host.ts"), /querySelector\([^)]*(canvas|pdf-page)/);
});

test("each reader has a single app target; only PDF loads PDF JS", () => {
  for (const format of ["pdf", "txt", "md"]) {
    const html = read(`apps/reader-${format}-wasm/index.html`);
    assert.equal((html.match(/rel="rust"/g) ?? []).length, 1);
    assert.equal(html.includes('/pdfEngine.js'), format === 'pdf');
    assert.equal(html.includes('/vendor/pdfjs/pdf.min.mjs'), format === 'pdf');
    const cargo = read(`apps/reader-${format}-wasm/Cargo.toml`);
    assert.match(cargo, /default-features = false/);
    assert.match(cargo, new RegExp(`features = \\["${format}"\\]`));
  }
});

test("reader state and persistence have no library signal access", () => {
  assert.match(read("src/state/app.rs"), /#\[cfg\(feature = "library"\)\]\s+pub library: LibraryState/);
  for (const file of ["src/runtime/reader.rs", "src/services/document/flush.rs", "src/services/document/open/shelf.rs", "src/effects/reader/reading_progress.rs"]) {
    assert.doesNotMatch(read(file), /state\.library|persist_library|load_library/);
  }
  assert.doesNotMatch(read("crates/library-core/Cargo.toml"), /^reader-core\s*=/m);
});
