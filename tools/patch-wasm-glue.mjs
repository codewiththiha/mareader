// wasm-bindgen 0.2.127 `--target web` keeps the instance here, not only in
// `wasm` (crates/cli-support/src/js/mod.rs, generate_web_loading):
//
//   let wasmModule, wasmInstance, wasm;
//   function __wbg_finalize_init(instance, module) {
//     wasmInstance = instance;
//     wasm = instance.exports;
//     wasmModule = module;
//   }
//   async function __wbg_init(...) {
//     if (wasm !== undefined) return wasm;
//   }
//
// Nulling the export object, or only `wasm`, leaves `wasmInstance`. The ES
// module map never unloads a blob import (wasm-bindgen #5339), so that
// binding keeps the linear memory after unmount. A new blob URL per open
// then adds another instance. That is the ratchet.
//
// `release()` runs in the module, so it can drop those bindings. It does
// not instantiate a replacement. Call it only after dispose: dispose still
// needs `wasm`. The next mount is a new blob module, whose `wasm` starts
// undefined, so `init` builds a new instance instead of returning this one.

import { readFileSync, writeFileSync } from "node:fs";

const CACHED = /\blet (cached[A-Za-z0-9_]+)\b/g;

function declared(source, name) {
  return new RegExp(`\\b(?:let|var)\\b[^;]*\\b${name}\\b`).test(source);
}

export function patchGlueSource(source) {
  if (!declared(source, "wasm")) {
    throw new Error(
      "wasm-bindgen glue has no module-scope `wasm`; release() would not free the instance",
    );
  }
  if (/\bexport function release\s*\(/.test(source)) return source;

  const cached = [...source.matchAll(CACHED)].map((match) => match[1]);
  const init = source.match(/\bfunction (__wbg_init|init)\b/)?.[1] ?? null;
  const lines = ["export function release() {"];
  // Each write is its own try. One sealed binding must not skip the instance.
  const drop = (stmt) => lines.push(`  try { ${stmt} } catch (_) { /* sealed */ }`);
  if (declared(source, "wasmInstance")) drop("wasmInstance = undefined;");
  if (declared(source, "wasmModule")) drop("wasmModule = undefined;");
  drop("wasm = undefined;");
  for (const name of cached) drop(`${name} = null;`);
  // #3130: the externref heap is not reset by init, and its slots keep JS
  // values that can still see the old memory. `let heap` is the web glue.
  if (declared(source, "heap")) {
    drop("heap = new Array(1024).fill(undefined);");
    drop("heap.push(undefined, null, true, false);");
  } else if (/\bconst heap\b/.test(source)) {
    drop("for (let i = 0; i < heap.length; i++) heap[i] = undefined;");
  }
  if (declared(source, "heap_next")) {
    drop("heap_next = heap.length;");
  }
  if (declared(source, "numBytesDecoded")) drop("numBytesDecoded = 0;");
  if (declared(source, "WASM_VECTOR_LEN")) drop("WASM_VECTOR_LEN = 0;");
  // Older glue stored the compiled module here. 0.2.127 web glue does not.
  if (init) {
    drop(`${init}.__wbindgen_wasm_module = undefined;`);
  }
  lines.push("}", "");
  return `${source.replace(/\s*$/, "\n")}${lines.join("\n")}`;
}

export function patchGlueFile(path) {
  const source = readFileSync(path, "utf8");
  const patched = patchGlueSource(source);
  if (patched !== source) writeFileSync(path, patched);
}
