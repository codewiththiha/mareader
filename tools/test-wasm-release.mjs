// The unmount wasm-bindgen does not generate. 0.2.127 web glue holds the
// instance in `wasmInstance`. Nulling `wasm` alone leaves that binding, and
// the module map keeps the linear memory. This checks release() drops it.

import assert from "node:assert/strict";
import { patchGlueSource } from "./patch-wasm-glue.mjs";

const current = `
let wasmModule = { code: true };
let wasmInstance = { held: true };
let wasm = { memory: { buffer: new ArrayBuffer(32) } };
let cachedUint8Memory0 = new Uint8Array(wasm.memory.buffer);
let cachedInt32Memory0 = new Int32Array(wasm.memory.buffer);
let heap = new Array(1024).fill(undefined);
heap.push(undefined, null, true, false);
heap.push(wasm);
let heap_next = heap.length;
function __wbg_init() {
  if (wasm !== undefined) return wasm;
  return wasm;
}
export function peek() { return wasm; }
export function peekInstance() { return wasmInstance; }
export function peekWasmModule() { return wasmModule; }
export function peekBytes() { return cachedUint8Memory0; }
export function peekInts() { return cachedInt32Memory0; }
export function peekHeapSlot() { return heap[1028]; }
export function peekHeapNext() { return heap_next; }
export default __wbg_init;
`;

const patched = patchGlueSource(current);
assert.equal(patched.includes("export function release("), true);
assert.equal(patched.includes("wasmInstance = undefined"), true);
assert.equal(patched.includes("wasmModule = undefined"), true);
assert.equal(patched.includes("wasm = undefined"), true);
assert.equal(patched.includes("cachedUint8Memory0 = null"), true);
assert.equal(patched.includes("cachedInt32Memory0 = null"), true);
assert.equal(patched.includes("heap = new Array(1024)"), true);
assert.equal(patched.includes("heap_next = heap.length"), true);
assert.equal(patchGlueSource(patched), patched);

const url = `data:text/javascript,${encodeURIComponent(patched)}`;
const glue = await import(url);
assert.equal(typeof glue.release, "function");
assert.ok(glue.peekInstance());
assert.ok(glue.peek());
glue.release();
assert.equal(glue.peekInstance(), undefined);
assert.equal(glue.peekWasmModule(), undefined);
assert.equal(glue.peek(), undefined);
assert.equal(glue.peekBytes(), null);
assert.equal(glue.peekInts(), null);
assert.equal(glue.peekHeapSlot(), undefined);
assert.equal(glue.peekHeapNext(), 1028);

const comma = "let wasmModule, wasmInstance, wasm;\nfunction __wbg_init() {}\nexport default __wbg_init;\n";
const commaPatched = patchGlueSource(comma);
assert.equal(commaPatched.includes("wasmInstance = undefined"), true);
assert.equal(commaPatched.includes("wasmModule = undefined"), true);
assert.equal(commaPatched.includes("wasm = undefined"), true);

const older = `
let wasm = { memory: { buffer: new ArrayBuffer(8) } };
let cachedUint8Memory0 = new Uint8Array(wasm.memory.buffer);
const heap = [wasm];
function init() { return wasm; }
init.__wbindgen_wasm_module = { cached: true };
export function peek() { return wasm; }
export function peekBytes() { return cachedUint8Memory0; }
export function peekHeap() { return heap[0]; }
export function peekModule() { return init.__wbindgen_wasm_module; }
export default init;
`;
const olderPatched = patchGlueSource(older);
assert.equal(olderPatched.includes("wasmInstance = undefined"), false);
assert.equal(olderPatched.includes("wasm = undefined"), true);
const olderUrl = `data:text/javascript,${encodeURIComponent(olderPatched)}`;
const olderGlue = await import(olderUrl);
olderGlue.release();
assert.equal(olderGlue.peek(), undefined);
assert.equal(olderGlue.peekBytes(), null);
assert.equal(olderGlue.peekHeap(), undefined);
assert.equal(olderGlue.peekModule(), undefined);

assert.throws(() => patchGlueSource("export function mount() {}\n"), /wasm/);

console.log("wasm release ok");
