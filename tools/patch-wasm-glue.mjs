// wasm-bindgen's web target keeps the instance in a module-scope `let wasm`
// and keeps typed-array views of `memory.buffer` in `cached*Memory0`.
// Nulling the exported functions does not clear those bindings, and the ES
// module map keeps the module record after `URL.revokeObjectURL`, so the
// linear memory stays reachable. There is no generated destroy (wasm-bindgen
// #3818, #5339). `release()` is the unmount: it runs in the same module, so
// it can drop the bindings the exports cannot see.
//
// Call it only after the artifact's own dispose. Dispose still needs `wasm`.

import { readFileSync, writeFileSync } from "node:fs";

const CACHED = /\blet (cached[A-Za-z0-9_]+)\b/g;

export function patchGlueSource(source) {
  if (!/\blet wasm\b/.test(source)) {
    throw new Error(
      "wasm-bindgen glue has no module-scope `let wasm`; release() would not free the instance",
    );
  }
  if (/\bexport function release\s*\(/.test(source)) return source;

  const cached = [...source.matchAll(CACHED)].map((match) => match[1]);
  const init = source.match(/\bfunction (__wbg_init|init)\b/)?.[1] ?? null;
  const lines = ["export function release() {", "  wasm = undefined;"];
  for (const name of cached) lines.push(`  ${name} = null;`);
  if (/\b(?:let|const|var) heap\b/.test(source)) {
    lines.push(
      "  if (typeof heap !== \"undefined\" && Array.isArray(heap)) { for (let i = 0; i < heap.length; i++) heap[i] = undefined; }",
    );
  }
  if (init) {
    lines.push(
      `  try { ${init}.__wbindgen_wasm_module = undefined; } catch (_) { /* init already gone */ }`,
    );
  }
  lines.push("}", "");
  return `${source.replace(/\s*$/, "\n")}${lines.join("\n")}`;
}

export function patchGlueFile(path) {
  const source = readFileSync(path, "utf8");
  const patched = patchGlueSource(source);
  if (patched !== source) writeFileSync(path, patched);
}
