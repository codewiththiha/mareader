# Phase 1: the session boundary

Leaving the reader must drop the reader heap. pdf.js must not load on a shelf boot. Resume, gloss, and covers must be flushed through the stores that already exist before the page dies. A second open recycles; it does not open into the heap that already holds a book. The reader in this phase is still the fat binary: every format stays linked. Panes, a host/format split, a slot grid, and deleting the search-index retention are later phases. This file is the spec for the boundary. `docs/split-wasm-modules.md` stays the overview.

## Why the drop is a document navigation

The reclaim is the death of the document, which is the same class of reclaim as Reload Window. That already works. A living page does not give the wasm heap back: the linear memory never shrinks, freed blocks return to the module's free list, and WebKit returns arenas to the OS only under pressure. Dropping every JS reference to the instance and letting GC collect it is the theoretical in-page path. It is not the path this pass takes, and it is not testable here.

wasm-bindgen's glue is a singleton. Trunk replaces the rust link in `index.html` with one module script that calls `init` once and starts the app. A second `init` of that glue in the same page does not build a second heap, and nothing in the page can drop the first instance while the glue, a memory view, or a typed-array alias of `memory.buffer` still points at it. An in-page swap was rejected for this pass. An iframe around the app was rejected for the same reason a second instance was: the glue still has to be born and die with a document, and the chrome is inside the wasm, not around it.

Two compiled artifacts would not be the reclaim either. `library.wasm` and `reader.wasm` would each have their own linear memory, which is what makes drop-one-heap possible, but only if each artifact is its own instance and the one you are leaving is actually destroyed. Destroying it is the navigation. A second file only lowers the library instance's initial data-segment floor. It does not return the bytes a book pushed the heap to. This phase does not add a second Trunk target, a second feature, or a second `trunk build` in `beforeBuildCommand`. `release.yml` is the only Trunk path, and push CI does not run it. An untested double build does not belong in the release hook.

So the reading session is another document lifetime of the binary Trunk already emits. The shelf boots at `/?`. The reader boots at `/?session=reader`. Both are the entry URL (`/`, `index.html`), which Trunk and Tauri serve. The path `/reader` is not used. Tauri's asset protocol does not SPA-fallback, so a full GET of `/reader` can 404. `location.assign("/reader")` is forbidden. The query is the session.

## What each lifetime holds

The library page mounts the shelf, the shelf effects, and nothing that reads a document. No `ReaderPage`, no reader effects, no router. Its status stays idle unless an open is waiting on an import, so a drop on it imports. pdf.js is not on the shelf's script list.

The reader page mounts the reader, the reader effects, and the paper settings. It does not install the shelf's startup rescan or cover backfill: those would fight the book that is about to open, and the shelf runs them again when the reader page dies. `paper_settings` publishes the blend synchronously and must run before the handoff open. `apply_theme` may call `refresh_theme`; that call is a no-op while `window.PDFReader` is absent, so a shelf boot that has not loaded pdf.js does not panic.

Both lifetimes install the OS-file listener. The once-flag resets because the page reloaded. A double-clicked file is collected by `take_pending_file`, which is a Tauri command, not a pdf.js call. The shelf must be able to pull it without the engine loaded, so the Rust wrapper invokes the command directly when `PDFReader` is absent. When the engine is present, the existing bridge path stays.

The router is gone from the mounted tree. `leptos_router` stays in the manifest so this pass does not rewrite `Cargo.lock`. Nothing imports it.

## Boot

`public/session/boot.ts` is bundled to `public/sessionBoot.js` (ESM, not an IIFE: it top-level-awaits) and copied like the other generated engines. Trunk's watcher ignores it, same as `pdfEngine.js`. `index.html` loads it as a module script before the rust link. Trunk replaces that link with a module script in place. Module scripts run in order, and a top-level await in the earlier one finishes before the later one evaluates. That is the gate. The wasm module must not start until the boot object exists, and a reader boot must not start until the engines it needs have been imported.

The boot script reads `location.search` and `sessionStorage["mareader.handoff"]` and sets `window.__MAREADER_BOOT`. The key uses a dot. A colon would look like a window event, and `tools/check-events.ts` fails a raw `mareader:` or `pdfreader:` literal outside the two event tables. This protocol is not a `CustomEvent`. It does not join `src/events.rs`.

- Shelf: no engine imports. The boot object says `library`. Wasm starts. pdf.js is not loaded.
- Reader, with a handoff: import `/vendor/pdfjs/pdf.min.mjs`, then `/pdfEngine.js`, then `/readerEngine.js`, then set the boot object, then let wasm start. The specifiers are not literals in the bundled output, so esbuild cannot inline pdf.js into the boot script.
- `?session=reader` with no usable handoff: strip the query and navigate to the shelf, and await a promise that never resolves so the wasm module on the doomed page does not start.

`ensureEngine()` on the boot object is the shelf's cover path. The cover queue calls it before `cover_data_url`. The JS side already destroys a one-shot document task; what it cannot do is unload the module. Loading the module for a cover is allowed. Loading it for a shelf boot that never asks for a cover is not. The reader open path awaits the same function before `engine::open`, so a script-order miss cannot open a PDF against a missing global.

An empty `unload` listener opts the page out of the back-forward cache. A cached document is a living document: its wasm heap is the leak this boundary exists to end. `pagehide` flushes. `unload` is the opt-out, not the flush.

## Handoff

The payload is `{ path, bookId? }` in `sessionStorage` under `mareader.handoff`. The shelf writes it and navigates. The reader page reads it once, removes it, and opens through the existing open path.

Before the navigation, flush. `flush_appearance_commit` so a slider that has not committed is in the settings signal, then `save_settings` immediately (the theme save is debounced, and the timer dies with the page). `flush_read_point` so a resume the progress effect has not written yet is in the library blob. `persist_library` and `persist_covers` so a shelf edit or a cover filed since the last drain is in the stores the next page loads. Gloss is already written on each stroke, keyed by row id. The next page's `load_*` is the read side. No new store, no new key.

Open from the shelf does not open the document and then destroy the instance. `open_at` sees a live boot object, flushes, and navigates. The reader page is the one that calls `open_book` or `open_path`. A thread-local marks that apply, so the open path does not hand off again. Without that flag the reader would reload forever.

Close is the same boundary in the other direction: flush, remove the handoff, `location.replace` to `/`. Replace, not push, so Back from the shelf does not restore a reader URL. Open pushes, so Back from the reader loads `/` as a new shelf document (bfcache is disabled, so that load is not the dead page). A second open while reading writes a new handoff and reloads. The URL is already `/?session=reader`, so `location.replace` of the same URL would not reload; the boot script reloads when the next URL is this URL. The old heap dies with the document. The new page opens the new book. That is the recycle. There is no in-heap ratchet.

A handoff whose row is gone falls through to `open_path` on the path that was stored. A reader URL with nothing stored never mounts a reader: the boot script has already left for the shelf.

`reload_app` clears the handoff key before `reload_window`. That helper still `replace_state`s to `/` and reloads. Parking on `/` is still load-bearing: a reload that kept `?session=reader` would boot the reader against an empty handoff and bounce. Clearing the key means a history write that fails still cannot resume a book the user asked to leave.

While an import task is unfinished, or a conflict is still waiting on an answer, the shelf does not navigate. The import's future lives in this instance; navigating would cancel it and drop the result. Status goes to Opening so the click is visible, the dock and the conflict modals stay mounted (they are not inside the content the Opening overlay replaces), and a waiter hands off when the queue is idle. A newer click bumps a generation counter and the older waiter stands down. Beats that land in the few hundred milliseconds of the navigation itself are not delivered to the dead shelf. The next shelf boot rescans watched folders. That race is accepted. Import progress is not bridged across the page death.

A drop uses document status, not the URL. Anything other than Ready is treated as the shelf and imports. The shelf never becomes Ready, so a drop there imports. A reader that has finished opening is Ready, so a drop there calls `open_path` and therefore recycles.

## What this phase does not change

The release artifact is still one `trunk build --release` into `dist`. `cargo check` of the default wasm target is still the CI proof. The reader still links every format. Cover generation still lives where it lives; the shelf just loads pdf.js on demand before asking for a cover. Search-index retention is untouched. There is no slot grid and no second instance in the page.

Host tests never see a boot object. `should_swap` is false, and open/close stay in-process. No new Rust tests, so the README count does not move. The web lane runs `node tools/test-session-boot.mjs` against the bundled handoff module: parse, the reader-without-handoff decision, and the reload-versus-navigate choice.
