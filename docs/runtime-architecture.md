# Runtime ownership

The application is now booted by `host/host.ts`, not by a Leptos router.

```
Tauri WebView
  host controller (routing, native IPC, one reader lifetime)
  library-wasm (persistent Leptos owner and library state)
  iframe (present only while reading)
    reader-pdf-wasm OR reader-txt-wasm OR reader-md-wasm
    reader selection environment
    PDF only: pdf.js, PDF engine, PDF worker and bake worker
```

## What changed

- Four executable app packages select independent Cargo feature sets. The
  root `mareader` package is now the reusable `reader_ui` library, not an app.
  Keeping the shared UI sources in `src/` avoids duplicating the existing
  reader and keeps the repository's source-contract tooling useful.
- `document-core` owns the format registry and filename policy. `library-core`
  no longer depends on `reader-core`. The latter re-exports document identity
  for compatibility with existing callers.
- Reader builds have **no `LibraryState` field**. Library modules, migrations,
  services and persistence operations are feature-gated out of those builds.
  OPEN carries one document's identity, resume point, settings and optional
  cover; it never carries library rows, shelves or file bytes.
- Reader progress, metadata, settings and cover images travel as snapshots.
  The persistent library alone updates and persists its rows. Independent
  copies retain their existing row-addressed progress/highlight semantics.
- The reader's one cover is a document signal, not a reference to a library
  cover map. Missing covers are generated on first read. Import-time backfill
  now retains cached art and uses format glyphs for missing art instead of
  loading PDFs into the persistent library.
- Markdown parsing/rendering and TXT parsing are selected by app features.
  Format-neutral heading projection lives in `reflow-core` so the PDF and TXT
  applications do not acquire the Markdown parser for its heading type.
- The rendering algorithms, page hosts, virtualizers, zoom, PDF engine and
  explicit reset sequence are reused rather than rewritten.

## Protocol and ordering

A version-1 envelope contains a payload and optional request ID. The host
transfers one MessagePort to the exact iframe window; the child verifies
source, origin, version and the number of ports. No file path is put in an
iframe URL. After the connection installs the native proxy, the selected
WASM application mounts and sends READY. Only then does the host send OPEN.

The manager has one active reader, a generation counter and a shared closing
barrier. Replacing a book invalidates an older open immediately, waits for
its disposal, and only then creates the next frame. Stale readers may flush
progress for their own book but cannot navigate or close their successor.

Normal disposal:

1. Claim the session generation, invalidating unresolved opens.
2. Flush progress and settings before resetting any document identity.
3. Reset the document, viewer, search, AI, gloss and paper state.
4. Await engine destruction and perform the existing sweeps.
5. Drop the Leptos mount handle, cleaning its owner and listeners.
6. Send DISPOSED with the disposal request ID.
7. Close MessagePorts, unsubscribe native events, remove the iframe, and
   clear host references before restoring library visibility and focus.

Startup has a 30-second deadline. Disposal has a 5-second fallback deadline:
if a broken reader cannot acknowledge, the host reports the failure and
removes the frame. This fallback cannot promise the last unacknowledged
position was saved. `pagehide` is a best-effort safety net, not a substitute
for acknowledged disposal.

## Native bridge

Only the host owns native reader subscriptions. Reader APIs use a scoped RPC
proxy over their MessagePort. File reads must match the configured path;
only the reader's required native commands, window methods and events are
admitted. Disposing the scope unlistens every event, including registrations
that resolve late. Rust-side Tauri listeners also retain and call their
unsubscribe handles when their reactive owner is cleaned up.

The iframe is a same-origin lifecycle boundary, **not a security sandbox**
for hostile code. Markdown keeps its existing raw-HTML-disabled renderer.
The host never accesses document canvases or a child's WASM instance.

## Builds and development

Use `npm run build` for a release frontend. `tools/build-runtimes.mjs` runs
Trunk once for the library host, then once per reader, assembling:

```
dist/index.html                 persistent host + library WASM
 dist/reader-pdf/index.html     PDF application + its own WASM
 dist/reader-txt/index.html     TXT application + its own WASM
 dist/reader-md/index.html      Markdown application + its own WASM
```

Shared static assets are copied to the root but executed only in the reader
that needs them. Hashed WASM and glue URLs use each target's own public URL.
Do not replace these builds with a workspace-wide frontend invocation:
Cargo feature unification would hide the boundaries being tested.

`npm run dev:frontend` builds the four development artifacts and serves them
on port 1420. Re-run it after Rust changes; this deliberately does not offer
partial hot reload that could leave three stale readers. Tauri's dev/release
hooks invoke these same commands. Direct `trunk serve` builds only the host
and is no longer a complete application development command.

## Validation

CI runs the existing Rust, native shell, TypeScript, engine and contract
checks; protocol/manager tests cover cancellation, rapid replacement,
double-close, final progress, and ten open/close cycles. A separate job
builds all four real release artifacts and runs Chromium against the built
WASM: real PDF rendering, TXT/Markdown with a mocked native filesystem,
iframe removal, and persistence of the original library DOM.

Desktop-only acceptance remains necessary on WKWebView and WebView2: native
file handoff, drag regions, traffic lights, OS close, and repeated large-PDF
scroll/zoom cycles. CI's Chromium test is not a measurement of native process
memory. Browser module caches and OS allocator high-water marks are not the
same as a retained document instance.

## Remaining dependency work

Runtime ownership is separated, but dependency minimization is not finished.
The shared UI still uses `pdf-engine`/`pdf-core` types and guarded helpers for
format-neutral document status, geometry, dialog and appearance concerns.
Consequently the library/TXT/Markdown Cargo graphs still contain those Rust
crates (without loading the PDF JavaScript environment). The library also
uses empty reader/chrome signals needed by the existing shared shell.
Extracting those neutral types and slimming the shell is follow-up work;
this implementation does not claim the ideal minimal library dependency
graph or measured memory savings. Shared CSS is likewise compiled once and
included in each realm; it is isolated by documents, not yet split by format.
