// Mount the real wasm bundle in Node, the way the webview does.
// CI compiles the frontend and runs the host tests, but neither ever MOUNTS
// the app: a panic on the library page's first frame is invisible to both —
// the bundle builds, the tests pass, and the reader gets a blank window whose
// only evidence is a line in a webview console nobody is watching. This is
// the check for that: jsdom stands in for the webview, localStorage is seeded
// with a library that resembles a real one (books, a book whose file is
// gone, a link row, a watched folder, a cover), the bundle is required — and
// for a bin crate, requiring runs `main` — and the script asserts that the
// shelf actually painted, with no panic on the way.
// It used to run in CI on every push and cost more than it caught: the mount
// itself is four seconds, but the wasm codegen that feeds it is a minute of a
// runner's time to re-prove what `cargo check --target wasm32-unknown-unknown`
// has already established. Run it by hand before a release, or after touching
// the startup path (the library's first frame, the measure-and-rescan, the
// storage read) — that is where a blank window comes from.
// Two passes, because the app has two environments:
//   node tools/wasm-smoke.mjs            plain browser: no window.__TAURI__,
//                                        so every shell call is skipped.
//   SMOKE_TAURI=1 node tools/wasm-smoke.mjs
//                                        a stubbed shell: measure_paths
//                                        answers honestly (the gone book is
//                                        gone), scan_folder walks an empty
//                                        folder, so the startup passes —
//                                        verify, then rescan — run for real.
// Prereqs, which nothing installs for you any more:
//   cargo build --target wasm32-unknown-unknown --bin pdf-reader
//   wasm-bindgen --target nodejs --out-dir smoke \
//     target/wasm32-unknown-unknown/debug/pdf-reader.wasm
//   printf '{"type":"commonjs"}\n' > smoke/package.json
//   npm install --no-save --no-package-lock jsdom
// The `wasm-bindgen` CLI has to be the version Cargo.lock pins or the glue it
// emits will not agree with the bundle. The package.json is there because the
// repo is an ES module and the nodejs glue is CommonJS: the directory it lands
// in has to say which of the two it is, or `require` reads the glue as a module
// and it dies on its own `exports` before a line of the app has run.

import { existsSync, readdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { JSDOM, VirtualConsole } from 'jsdom';

const WITH_TAURI = process.env.SMOKE_TAURI === '1';
const SMOKE_DIR = path.resolve('smoke');

// The seed: a library a reader could actually have. Shapes must match the
// serde wire format exactly (LibraryBlob/Row/Book/Fingerprint/Origin/Shelf/
// WatchedFolder are all camelCase; Row and Origin are tagged on "kind").

const fp = { size: 1000, mtimeMs: 1_700_000_000_000, headHash: 7 };

const book = (id, title, src, extra = {}) => ({
  kind: 'book',
  id,
  fp,
  title,
  format: 'pdf',
  origin: { kind: 'linked', src },
  addedMs: 1_700_000_000_000,
  lastReadMs: 0,
  page: 1,
  numPages: 12,
  ...extra,
});

const library = {
  books: [
    book('b1', 'Dune', '/home/demo/books/dune.pdf'),
    book('b2', 'Neuromancer', '/home/demo/books/neuromancer.pdf'),
    // The case the Find-again flow exists for: the row stays, the file is
    // gone, and the card paints the missing badge on the first frame.
    book('b3', 'Gone Book', '/home/demo/books/gone.pdf', { missing: true }),
    { kind: 'link', id: 'l1', name: 'Dune (alias)', target: 'b1', addedMs: 1_700_000_000_000 },
  ],
  shelves: [{ id: 's1', name: 'Favourites', books: ['b1', 'b3'] }],
  folders: [{ id: 'f1', root: '/home/demo/books', opts: { watch: true } }],
};

// One per book, and that is deliberate. A cover the library does not hold is a
// cover the shelf renders, and rendering one is the TS engine plus a canvas
// paint — neither of which a jsdom stand-in has, and neither of which is what
// this check is asking. It is asking whether the app MOUNTS: whether the first
// frame builds a shelf. Seeding every book's art is what keeps the answer about
// the mount instead of about a cover queue with no engine behind it.
const cover = {
  dataUrl: 'data:image/gif;base64,R0lGODlhAQABAAAAACw=',
  width: 120,
  height: 160,
};
const covers = Object.fromEntries(
  ['/home/demo/books/dune.pdf', '/home/demo/books/neuromancer.pdf', '/home/demo/books/gone.pdf'].map(
    (path) => [path, cover],
  ),
);

// Settings are deliberately NOT seeded: the defaults path is the one a fresh
// install takes, and every added field must survive an old stored blob —
// both are serde defaults, and a missing default is a mount the reader never
// gets past.

// The environment. jsdom is the webview; the shims below are only for APIs
// jsdom lacks that every real webview has — a shim here must never stand in
// for something the bundle should find missing.

const errors = [];
const notes = [];

// Collect on NODE's console, not jsdom's: console_error_panic_hook — where a
// Rust panic lands — writes through the wasm-bindgen glue straight to the
// host console, and so does everything the virtual console forwards. The
// hook has to be on before the JSDOM is built, so sendTo binds the hooked
// methods and page logs and wasm panics arrive through the one door.
const hook = (level, sink) => {
  const original = console[level].bind(console);
  console[level] = (...args) => {
    sink.push(
      args
        .map((a) => (typeof a === 'string' ? a : safeStringify(a)))
        .join(' '),
    );
    original(...args);
  };
};
hook('error', errors);
hook('warn', notes);

const virtualConsole = new VirtualConsole();
virtualConsole.on('jsdomError', (e) => {
  errors.push(`jsdomError: ${e && (e.stack || e.message)}`);
});
virtualConsole.forwardTo(console); // jsdom >= 27; sendTo on older

function safeStringify(value) {
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}

const dom = new JSDOM('<!doctype html><html><body></body></html>', {
  url: 'http://localhost:1420/',
  pretendToBeVisual: true, // requestAnimationFrame, like a visible window
  virtualConsole,
});
const { window } = dom;

// Present in every webview the app ships in, absent from jsdom.
class NoopObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
  takeRecords() {
    return [];
  }
}
window.ResizeObserver ??= NoopObserver;
window.IntersectionObserver ??= NoopObserver;
window.Element.prototype.scrollIntoView ??= function scrollIntoView() {};
window.HTMLElement.prototype.scrollIntoView ??= function scrollIntoView() {};
window.fetch ??= () =>
  Promise.resolve(new Response('', { status: 404, statusText: 'Not Found' }));
window.Response ??= Response;
// No media queries in jsdom, and the app asks the window one: a query answered
// "no" is a query a webview would have answered, and the alternative is a
// TypeError on the first frame — a fact about the stand-in, not about the app.
window.matchMedia ??= (query) => ({
  matches: false,
  media: query,
  onchange: null,
  addListener() {},
  removeListener() {},
  addEventListener() {},
  removeEventListener() {},
  dispatchEvent: () => false,
});

if (WITH_TAURI) {
  // The shell, stubbed at the contract the frontend declares in
  // crates/tauri-bridge: measure answers honestly for the seed (the gone
  // book stays gone, the rest measure to the fingerprint they carry, so the
  // startup pass changes nothing), a walk finds an empty folder, and every
  // window method resolves to a benign answer.
  const ANY_METHOD = () => Promise.resolve(null);
  window.__TAURI__ = {
    core: {
      invoke: (cmd, args) => {
        notes.push(`invoke: ${cmd}`);
        switch (cmd) {
          case 'verify_paths':
            return Promise.resolve(
              (args?.paths ?? []).map((p) => ({
                path: p,
                exists: !p.endsWith('gone.pdf'),
                size: fp.size,
                mtimeMs: fp.mtimeMs,
                headHash: fp.headHash,
              })),
            );
          case 'scan_folder':
            return Promise.resolve([]);
          default:
            return Promise.resolve(null);
        }
      },
    },
    event: { listen: () => Promise.resolve(() => {}) },
    window: {
      getCurrentWindow: () =>
        new Proxy(
          {},
          {
            get: () => ANY_METHOD,
          },
        ),
    },
    dialog: { open: () => Promise.resolve(null) },
  };
}

// The bundle reaches for these as bare globals, the way it would off
// window.* in the webview.
// Defined rather than assigned, because Node has grown several of them as
// getter-only accessors of its own — `navigator` since 21 — and an assignment
// to one throws in a module, which is strict. The throw has nothing to do with
// the app and everything to do with the runner, and it kills the check before
// the bundle is even required.
const defineGlobal = (name, value) => {
  Object.defineProperty(globalThis, name, {
    value,
    writable: true,
    configurable: true,
    enumerable: true,
  });
};

defineGlobal('window', window);
defineGlobal('document', window.document);
defineGlobal('navigator', window.navigator);
defineGlobal('location', window.location);
defineGlobal('localStorage', window.localStorage);
defineGlobal('sessionStorage', window.sessionStorage);
defineGlobal('history', window.history);
for (const name of [
  'Node',
  'Element',
  'HTMLElement',
  'HTMLInputElement',
  'HTMLCanvasElement',
  'Event',
  'CustomEvent',
  'MouseEvent',
  'KeyboardEvent',
  'DragEvent',
  'FocusEvent',
  'WheelEvent',
  'PointerEvent',
  'InputEvent',
  'ResizeObserver',
  'IntersectionObserver',
  'MutationObserver',
  'DOMParser',
  'getComputedStyle',
  'requestAnimationFrame',
  'cancelAnimationFrame',
  'matchMedia',
  'Image',
  'Response',
  'fetch',
  'Headers',
  'Request',
  'AbortController',
  'File',
  'FileReader',
  'Blob',
  'URL',
  'URLSearchParams',
]) {
  if (name in window) defineGlobal(name, window[name]);
}

// And then everything else the window owns that Node's global does not, because
// the list above is a list a hand keeps and the bundle asks for more of it with
// every web-sys feature it turns on. `in` rather than an own-property read: a
// global Node already has is Node's, and replacing its timers or its console
// with jsdom's would change the runner under the check.
for (const name of Object.getOwnPropertyNames(window)) {
  if (name === 'globalThis' || name in globalThis) continue;
  const descriptor = Object.getOwnPropertyDescriptor(window, name);
  if (!descriptor) continue;
  let value;
  try {
    value = descriptor.get ? descriptor.get.call(window) : descriptor.value;
  } catch {
    continue; // a getter that needs a real webview: nothing to stand in for
  }
  if (value === undefined) continue;
  try {
    defineGlobal(name, value);
  } catch {
    // The runner owns it and will not let go. Its own answer stays.
  }
}

// Two things a copy cannot give, and both are about the same fact: inside the
// bundle, `window` is not a variable but the global object. `web_sys::window()`
// is `js_sys::global().dyn_into::<Window>()`, which the glue spells as
// `globalThis instanceof Window`, and the app's first frame reaches a document
// through it — so a global that is not a Window is a panic before a single view
// is built, which is a fact about the runner and not about the app.
for (const name of ['addEventListener', 'removeEventListener', 'dispatchEvent']) {
  if (typeof window[name] === 'function') defineGlobal(name, window[name].bind(window));
}
const RealWindow = window.Window;
defineGlobal('Window', function Window() {
  throw new TypeError("Failed to construct 'Window': Illegal constructor");
});
Object.defineProperty(globalThis.Window, Symbol.hasInstance, {
  configurable: true,
  value: (instance) =>
    instance === globalThis ||
    Function.prototype[Symbol.hasInstance].call(RealWindow, instance),
});

window.localStorage.setItem('pdfreader.library.v3', JSON.stringify(library));
window.localStorage.setItem('pdfreader.covers.v1', JSON.stringify(covers));

// An async panic after a successful mount is still a bug the reader meets;
// keep the process alive to collect it instead of dying on the spot.
process.on('unhandledRejection', (reason) => {
  errors.push(`unhandledRejection: ${reason && (reason.stack || reason.message || reason)}`);
});

// The mount. For a bin crate the bound module runs `main` on require, so this
// call IS the app starting: if the first frame panics, it throws here, and
// console_error_panic_hook has already put the message in `errors`.

const glueName = existsSync(SMOKE_DIR)
  ? readdirSync(SMOKE_DIR).find((f) => f.endsWith('.js') && !f.endsWith('.d.ts'))
  : undefined;
if (!glueName) {
  console.error(`no bound module in ${SMOKE_DIR} — run wasm-bindgen --target nodejs first`);
  process.exit(2);
}

let mountThrew = null;
try {
  createRequire(import.meta.url)(path.join(SMOKE_DIR, glueName));
} catch (e) {
  mountThrew = e;
}

// Let spawned futures, effects and the (stubbed) shell calls settle, then
// read what the webview would be showing.
await new Promise((r) => setTimeout(r, 1000));

const body = window.document.body;
const html = body.innerHTML;
const panicked = errors.some((e) => /panicked at|RuntimeError: unreachable/.test(e));
const painted = html.includes('Dune') && html.includes('data-tauri-drag-region');

console.log(`\n=== wasm smoke (${WITH_TAURI ? 'stubbed shell' : 'plain browser'}) ===`);
console.log(`mount threw:      ${mountThrew ? mountThrew.message : 'no'}`);
console.log(`body children:    ${body.children.length}`);
console.log(`body html bytes:  ${html.length}`);
console.log(`shelf painted:    ${painted ? 'yes' : 'NO'}`);
console.log(`panics:           ${panicked ? 'YES' : 'none'}`);
if (notes.length) console.log(`notes:\n  ${notes.join('\n  ')}`);
if (errors.length) console.log(`errors:\n  ${errors.join('\n  ')}`);
console.log(`body snippet:     ${html.slice(0, 300)}`);

const ok = !mountThrew && !panicked && painted;
console.log(ok ? 'SMOKE PASS' : 'SMOKE FAIL');
process.exit(ok ? 0 : 1);
