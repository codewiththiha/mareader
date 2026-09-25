# The runtime split (Phase 2): how the three runtimes are built, loaded, and disposed

This document is the Phase 2 §1 record: the actual runtime technology, decided
from the existing build (Trunk CSR, one `index.html` target, wasm-bindgen
`--target web` glue, Trunk 0.21.14 in CI), not assumed.

## WASM targets

Three independently loadable WASM artifacts, one per runtime lifetime:

| Runtime | Package (Cargo) | Entry HTML | Artifact |
| --- | --- | --- | --- |
| Shell | `mareader` (workspace root) | `index.html` | `mareader.js` + `mareader_bg.wasm` |
| Reader | `reader-runtime` | `reader.html` | `reader.js` + `reader_bg.wasm` |
| Library | `library-runtime` | `library.html` | `library.js` + `library_bg.wasm` |

Each runtime is a separate `[[bin]]` in its own package, so each artifact
contains only its own module tree: the shell artifact has no reader or
library code in it, and the library artifact has no `ReaderState` (the
compiler cannot even name it — the type lives in `app-state`, but no library
crate imports it; a compile error is the kill switch, not a review rule).

`reader.html` / `library.html` are standalone loadable pages: they boot their
runtime against the real production entry (`run_standalone`), which is the
build-level proof that each artifact stands alone. The packaged app and
`trunk serve` use `index.html` only; the two extra pages are built by their
own Trunk config files (`reader.Trunk.toml`, `library.Trunk.toml`, each with
`filehash = false` so the shell can name them) and merged into `dist/` by
`tools/build-dist.sh`.

## Entry modules

- Shell: `src/main.rs` (`mareader` bin) — mounts the shell: route state, the
  runtime manager, the persistent overlays, the diagnostics surface, the
  bridge the runtimes call into.
- Reader: `crates/reader-runtime/src/main.rs` — reads its launch descriptor
  and mounts the reader session (see below); `run_standalone` boots without a
  shell.
- Library: `crates/library-runtime/src/main.rs` — same shape.

## Loader mechanism

The shell loads a runtime module on demand with a dynamic `import()` of the
artifact's glue JS (`/reader.js`, `/library.js` — wasm-bindgen `--target web`
glue, which self-initializes: module evaluation instantiates the WASM and
runs the bin's `main`). The import is issued through one inline helper
(`dyn_import`) so the Rust side owns a `Promise` of the module namespace,
whose exports are the runtime's mounted-session API.

A loaded module stays in the realm's module map — that is compiled-code
caching, which the phase explicitly allows. The live runtime is NOT the
module: it is the session object the module creates per `mount` call.

## Mount targets

The shell owns one mount target:

```html
<body>
  … shell-owned overlays (noise, drag feedback) …
  <div id="runtime-host"></div>   <!-- exactly one active runtime mounts here -->
</body>
```

The manager replaces the host's content deliberately: the outgoing runtime's
DOM is removed before the incoming runtime mounts. A runtime never reaches
outside its mount root; `document.body`-level chrome belongs to the shell.

## Instance creation

`RuntimeManager::start_reader(state, LaunchDocument)` /
`start_library(state)`:

1. Dispose the active runtime first (below) and await its completion.
2. `dyn_import("/reader.js")` (cached after the first start — compiled code).
3. Read the exported `mareader_reader_start(host, launch_json)` function from
   the module namespace.
4. Call it: inside the reader artifact this creates a NEW session — a fresh
   reactive ownership root (`mount_to` into the host), a fresh `ReaderState`
   + `ReaderRuntime` (the Phase 1 owner, begin_mount → mark_ready inside the
   session scope), fresh effects/listeners, the engine session — all owned by
   that session's scope.
5. The call returns a session id; the shell records
   `Slot::Reader { id, module }` (§5 enum identity — `None`/`Starting`/
   `Library`/`Reader`, no stringly state).

The launch data crossing the boundary is a serialized
`LaunchDocument` (`book_id`, `path`, `resume_page`, `saved_fraction`,
`blend_override`) — stable, minimal, serializable (§13/§14). The reader
obtains everything else through its own services.

## Instance disposal

`ReaderRuntimeHandle::dispose()` → the module's exported
`mareader_reader_dispose(session_id)`:

1. The session's ownership root is unmounted — Leptos runs the scope
   cleanups, whose FIRST-registered cleanup is the Phase 1
   `ReaderRuntime::dispose` chain (flush → registry take →
   engine destroy awaited → sweeps → virtualizer disposal →
   finish_dispose(generation)).
2. The exported function awaits that tail's completion (the dispose-complete
   diagnostics beat) before resolving its `Promise`.
3. The shell drops the session handle and clears the host. The library
   runtime (if being replaced) disposes the same way; its live state dies
   with the session and the next `start_library()` seeds a fresh one from
   storage.

The manager never starts a new runtime until the previous runtime's dispose
promise resolved (§5). A module's linear memory stays reserved after its
session ends — that is the compiled-code cache, and it is the same semantics
Phase 0 measured (`log_heap`): what must not survive is live state, and it
does not (the browser suite's `at_baseline` gates it).

## Cross-runtime communication

One narrow channel, JSON-serialized both ways (§14):

- runtime → shell: `window.__mareaderShell` — installed by the shell before
  any runtime loads; typed methods taking JSON strings
  (`openDocument`, `navigateLibrary`, `readPoint`, `saveSettings`,
  `saveLibrary`, `saveCovers`, `saveCover`, `docStatus`, `publishDigest`,
  `reload`, `resolveLaunch`).
- shell → runtime: the module's exported functions (`…_start`,
  `…_dispose`, `…_command` for in-session commands such as an
  OS drop while the reader is active).

Standalone pages install a storage-backed shell substitute so the same
entry code runs unhosted; the bridge is a trait (`ShellApi`) in `app-state`
with exactly these two implementations.

## Asset loading

`styles.css`, `public/vendor` (pdf.js), `pdfEngine.js`, `readerEngine.js`,
`bake.worker.js` are referenced by all three HTML entries, so each build
emits them; the merged `dist/` holds one copy. All three runtimes live in
the same JS realm and same origin: DOM, CSS custom properties (the shell
paints the durable theme on `<html>`), `localStorage` (one browser store;
each runtime touches only its own keys), Tauri APIs and window DOM events
are shared by the platform, while reactive state, WASM linear memory and
statics are per-artifact. That split — platform-shared, state-isolated — is
the boundary the rest of this migration keeps honest.
