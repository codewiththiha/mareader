// The unmount wasm-bindgen does not generate. Nulling exports leaves
// `let wasm` and the cached memory views alive, which is the leak. This
// imports a patched glue and checks release() actually drops those bindings.

import assert from "node:assert/strict";
import { patchGlueSource } from "./patch-wasm-glue.mjs";

const fixture = `
let wasm = { memory: { buffer: new ArrayBuffer(32) } };
let cachedUint8Memory0 = new Uint8Array(wasm.memory.buffer);
let cachedInt32Memory0 = new Int32Array(wasm.memory.buffer);
const heap = [wasm, cachedUint8Memory0];
function __wbg_init() { return wasm; }
__wbg_init.__wbindgen_wasm_module = { cached: true };
export function peek() { return wasm; }
export function peekBytes() { return cachedUint8Memory0; }
export function peekInts() { return cachedInt32Memory0; }
export function peekHeap() { return heap[0]; }
export function peekModule() { return __wbg_init.__wbindgen_wasm_module; }
export default __wbg_init;
`;

const patched = patchGlueSource(fixture);
assert.equal(patched.includes("export function release("), true);
assert.equal(patched.includes("wasm = undefined"), true);
assert.equal(patched.includes("cachedUint8Memory0 = null"), true);
assert.equal(patched.includes("cachedInt32Memory0 = null"), true);
assert.equal(patchGlueSource(patched), patched);

const url = `data:text/javascript,${encodeURIComponent(patched)}`;
const glue = await import(url);
assert.equal(typeof glue.release, "function");
assert.ok(glue.peek());
assert.ok(glue.peekBytes());
glue.release();
assert.equal(glue.peek(), undefined);
assert.equal(glue.peekBytes(), null);
assert.equal(glue.peekInts(), null);
assert.equal(glue.peekHeap(), undefined);
assert.equal(glue.peekModule(), undefined);

assert.throws(() => patchGlueSource("export function mount() {}\n"), /let wasm/);

console.log("wasm release ok");
