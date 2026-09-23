# Phase 2: format instances

The reader page from phase 1 stays the host. It stops being the thing that holds the book. One slot, the div that already exists (`#viewer-slot`). The book is a format instance: `pdf.wasm`, `text.wasm`, or `md.wasm`. Switching books drops that instance and mounts another. Closing the book still navigates to `/`, which drops the host and the instance with the document. Sidebar and appearance cross a bridge of copied values. Search-index retention is deleted, because the instance it was retained for is gone.

`docs/split-wasm-modules.md` stays the overview. `docs/phase-1-session-boundary.md` stays the session boundary. This file is the spec for the format instance. It does not reopen the session boundary, and it does not start the slot grid, the cover-generation session, or `memory.discard`.

## Why a format switch is not another page load

Phase 1 reloads the page for a second open because the Trunk glue is a singleton and the host heap cannot be dropped in place. That still stands for the host. A format switch is a different problem: the host has to stay, and only the book has to die.

A format module is not the Trunk glue. It is a cdylib with its own wasm-bindgen glue, evaluated fresh per mount. The host glue is never re-inited. Re-init of the same glue URL is still a lie: the ES module map returns the first evaluation, and that evaluation's `wasm` binding is already set, so a second `init` does not build a second heap. The loader does not `import()` the glue by its stable URL. It fetches the glue as text, imports that text from a blob URL, and calls `init` with the stable wasm URL. The blob URL is a new module record, so the glue's `wasm` binding starts undefined and `init` builds a new instance. The wasm URL may stay in a `WebAssembly.Module` cache. That cache is code, not the heap. The instance is not cached. A typed-array view of `memory.buffer` keeps the `ArrayBuffer` alive, so the host must not hold one.

Drop nulls every export, every memory view, and the instance, then revokes the blob URL. The next mount is a new heap at that artifact's floor. Five switches do not climb. A climb inside one instance is still a leak; the drop does not excuse it.

An iframe around the format module was rejected in the overview. CSS variables would not inherit, and the chrome would sit on the wrong side of the frame. The format module calls `mount_to` on the slot div. It never calls `mount_to_body`. The host and the format module are two Leptos trees in one document. They do not share a context, and they do not share a heap.

## What each artifact holds

| Artifact | Lifetime | Holds | Must not hold |
| --- | --- | --- | --- |
| Host (Trunk bin, `data-bin="mareader"`) | The reader page | Title bar, rail geometry, menus, the empty slot, a copied snapshot | Document bytes, pdf.js, a block tree, a page host |
| `pdf.wasm` | One PDF book | pdf.js, the worker, the canvases, the search index, the PDF page hosts | A text block tree |
| `text.wasm` | One text book | The block tree, the reflow page hosts | A PDF document, pdf.js |
| `md.wasm` | One Markdown book | The block tree, the headings, the reflow page hosts | A PDF document, pdf.js |

The host bin enables no format feature, so the view modules are not in its artifact. Each format crate enables only its own feature. Shared layout code is linked into whichever artifact mounts a view. `pdf-engine` is referenced only from modules the PDF feature compiles, so the text and Markdown artifacts do not link it. A text mount never takes the PDF open arm, and the loader never imports pdf.js for it. Linking unused code would be a floor. Allocating a document would be a failure. This phase does both: the open arm is not in the artifact, and the loader does not fetch the engine.

The three crates do not depend on each other. The host does not depend on them. They depend on the app as an rlib, with one feature. Trunk keeps building the bin. `index.html` sets `data-bin="mareader"`. The app lib is `rlib` only, never `cdylib`. A cdylib on the app package would collide with the bin's wasm name, and Trunk would stop being the host.

## The one slot

`VIEWER_SLOT_ID` (`viewer-slot`) is the slot. No new DOM id. The host renders it empty. The format module is the only thing that mounts into it. No grid, no second slot, no hover suggestion. Those are phase 3.

A second open while this page is already the reader does not navigate. `leave_for_reader` sees `is_reader()`, flushes, and asks the bootloader to drop the format instance and mount the new book's artifact into the same div. The host chrome stays. A second open from the shelf is still the phase 1 document navigation: the shelf has no host to keep. Close is still `enter_library`. The format instance dies because the document dies. The bootloader also drops its handle on `pagehide`, so a slow navigation cannot leave a key set, but the reclaim is the document death, not an in-page trick on the host.

`should_swap` is false inside a format instance. A thread-local, `IN_FORMAT`, is set for the mount and cleared on dispose. The format instance opens in-process. It must not see the host's boot object and navigate again. Without that flag a format mount would hand the open back to the host and the host would mount another instance.

Host tests have no boot object. `should_swap` stays false. They keep the in-process open path. That binary is not the shipped host. Do not "fix" them by requiring a format wasm.

## Bootloader

`window.__MAREADER_BOOT` grows `mountFormat` and `dropFormat`. This is not a `CustomEvent`. The key stays a property on the boot object. No `mareader:` or `pdfreader:` literal: `tools/check-events.ts` fails those outside the two event tables.

A reader boot stops importing pdf.js. Phase 1 loaded it for every reader page, including a text book. The PDF format mount imports `/vendor/pdfjs/pdf.min.mjs`, then `/pdfEngine.js`, then instantiates `formats/pdf.js`. Text and Markdown do not. The shelf's `ensureEngine` stays the cover path. Moving cover generation into a transient PDF session is phase 4. This phase does not take `pdf-engine` off the library crate.

URLs are relative (`formats/pdf.js`, `formats/pdf_bg.wasm`), so the asset protocol and `trunk serve` resolve the same files. The glue is fetched as text and imported from a blob, so the module map cannot hand back a live instance. `init` receives the wasm URL, not a cached instance.

The handle map has one key, `slot`. Phase 3 will key by slot id. This phase has one slot, and a second mount drops the first before it starts. The bootloader owns the handle. A Leptos component must not close over it. If it does, the drop is theatre.

Drop order, from the overview, applied to the one slot:

1. Flush. The host flushes before it asks. The format instance reports the page and any in-flight gloss stroke through the bridge, and the host writes the stores, before dispose. A debounced timer dies with the instance, so the write is immediate.
2. Dispose the Leptos owner for that mount. Cleanup runs here. `ResizeObserver`s disconnect. Portaled gloss nodes are removed here. A cleanup that runs after the instance is gone aborts the runtime.
3. Format teardown. PDF: the existing `destroy()` — sweep, release canvases, destroy the loading task, null the document — then drop the dynamic import of pdf.js and null `PDFReader`. Text and Markdown: disposing the owner drops the block `Vec`. There is no second engine.
4. Null the glue. Every export, the wasm-bindgen object, every `Uint8Array` over `memory.buffer`.
5. Drop the instance. The `Module` may stay, keyed by format, so the next open of the same format skips download and compile. Do not keep the instance.
6. Clear the handle key. If it is still set, the drop is not done.

Log `[mem]` from inside the instance at mount and at the start of teardown (`src/memory.rs`, the shape, not the OS number). The bootloader logs once the handle is null that the instance is gone.

## The bridge

Copies, because the heaps are not shared. One JSON envelope each way, on `window.__MAREADER_SLOT`. The host registers it. The format module calls it. Not a new glue crate per call. Not a new window event.

The types already exist and are already serde. Field names are a contract. Do not rename them.

- `Appearance`, inside `mareader.settings.v1`.
- `ReadPoint`, book id, path. `library-core`.
- `OutlineNode`. `reader-core`. PDF outline entries are already converted to this before the panel sees them.
- Paper color. A few numbers. The host writes `--pdf-paper` / `--tx-paper` on the slot. The format module does not need to know the other format to paint a variable it already paints.

Down: open payload (path, book id, resume page, fraction, the gloss marks for that row, appearance), zoom, go to page, search, an appearance patch, "the menu is open", slot size.

Up: title, page, page count, outline, paper color, format, status, a selection summary, and the page rect the floating title used to measure by walking a page-host id. The host does not grow a second copy of the `sp-` / `dp-` / `hp-` / `cont-` ids. The DOM contract stays inside the format module that owns the page.

The host writes the snapshot into the signals the chrome already reads: `document.format`, `document.title`, `document.num_pages`, `document.outline`, `document.status`, `viewer.page`. It does not write blocks. `texture_applies` stays "reader surface and not reflowable", which is now "the snapshot says PDF". The sidebar does not import a format widget.

Thumbnails: the host renders the existing `#thumb-scroll` empty. The PDF module mounts the thumbnail cells into that div. The canvases are created by the PDF instance, so dispose removes them. The host's cells stop calling `pdf_engine`. That call would build engine state in the host heap, which is the leak this phase exists to end. The two-deep render lane, the pixel ceiling, and the thumbnail LRU stay where they are. They keep a live instance's peak down. The split does not replace them. Text and Markdown publish no thumbnail panel; `thumbs_visible` already follows `reflowable()`, and the snapshot's format is what that reads.

Appearance stays on the title bar, in the host. Texture, the paper rebake, and the menu-open flag are a patch to the PDF instance. A text instance repaints CSS tokens. It does not rebake rasters. CSS variables inherit into the slot. Blend across panes is phase 3, because there is one slot here. A lone PDF keeps today's `blend_backdrop` behavior, driven inside that module.

Progress: the format module reports the page. The host writes the library blob. The format module does not hold `state.library.books`. Gloss strokes report through the bridge; the host persists by row id, as today. Teardown flushes before the instance is dropped.

Zoom, search, and page commands go to the format module. The host does not own a zoom scale and does not virtualize pages. The zoom controller stays next to the virtualizer it already drives.

The format instance's state is a reader, not a shelf. It loads settings (small, and the view already reads them) and the one row the payload names. It does not parse the library blob. The host already did, and the host is the one that writes it back.

## Search-index retention

The retention existed because the wasm heap never shrinks, so a rebuild on every reopen ratcheted a heap that outlived the book. The PDF instance is now mortal. A reopen extracts into a new heap that dies on the next close. That is the trade the retention was invented to avoid, and it is the right trade once the heap dies with the book.

`scope_to_document` no longer adopts. It clears. `destroy` clears the Rust index as well as the pdf.js document. `build_search_index` always extracts. The three tests in `crates/pdf-engine/src/api/search.rs` flip: a reopen does not adopt, a different book still finds nothing, a path is not a cache key. No new tests, so the README count stays 1,021.

Do not persist the index. Persisting it into the host would put the book back into the long-lived heap.

## Build

Push CI does not run Trunk. The wasm lane names the three crates or they rot:

```
cargo check -p format-pdf --target wasm32-unknown-unknown --locked
cargo check -p format-text --target wasm32-unknown-unknown --locked
cargo check -p format-md --target wasm32-unknown-unknown --locked
```

Three processes, not one invocation with three `-p` flags. One invocation unifies features, so a text mount would still compile the PDF open arm and the check would not prove the split. The host proof stays the existing `cargo check --target wasm32-unknown-unknown --locked` of the root package, which does not enable a format feature. Workspace clippy and `cargo test --workspace` unify features, because the format crates depend on the app with a feature. That build compiles the view. It is not the shipped host. The feature-off wasm check is the host proof. Both have to compile. `warnings = deny` already covers the feature-off check. No `allow(dead_code)` to paper over a module the host no longer calls.

`src-tauri/tauri.conf.json` `beforeBuildCommand` grows from `trunk build --release` to `node tools/build-frontend.mjs`. A shell script is not a `beforeBuildCommand` on Windows. The node script runs that one Trunk build, then builds the three cdylibs. Not a second Trunk target. The dist directory does not move. `frontendDist` stays `../dist`.

`tools/build-frontend.mjs` builds the three crates `--release` for `wasm32-unknown-unknown`, runs `wasm-bindgen --target web --out-name` at the lock's `wasm-bindgen` version (`0.2.127`), runs `wasm-opt -Oz` (never `-z`), and leaves `formats/pdf.js`, `formats/pdf_bg.wasm`, and the text and Markdown pairs in `dist/formats/`. The release job installs `wasm-bindgen-cli` at that version and `binaryen` so `wasm-opt` is on `PATH`. Trunk's own wasm-opt is not assumed to be on `PATH`.

Trunk's `[serve]` sets `no_spa`, and a `post_build` hook runs the same script into the staging dir, so `trunk serve` has the files. A missing glue is a 404, which the loader surfaces. It does not fall back to opening the book in the host, and it does not import the app page: that page starts with `<`, and importing it is `Unexpected token '<'` in a blob the debugger names `source`.

`public/session/format-loader.ts` holds the pure decisions (which artifact, the drop order, the one-slot rule). `tools/bundle-engine.mjs` emits it next to the handoff module, and the web lane runs `tools/test-format-loader.mjs` the way it runs the handoff test. The boot script imports the loader. Dynamic `import()` of the glue stays a variable, so esbuild cannot inline a format wasm into `sessionBoot.js`.

## Files

New:

- `docs/phase-2-format-instances.md` — this spec.
- `src/lib.rs` — rlib root. Same modules as the bin. No `main`.
- `src/format_runtime.rs` — `mount` / `dispose`, compiled only with a format feature. Sets `IN_FORMAT`, builds the one-row state, `mount_to`s the slot, runs the document open and the document effects the host page used to install.
- `src/slot.rs` — host-side bridge client. Feature-off. Registers `__MAREADER_SLOT`, applies a snapshot onto the chrome signals, sends a command.
- `crates/format-pdf`, `crates/format-text`, `crates/format-md` — cdylibs. Each exports `mount` and `dispose` and enables one feature. They do not import each other.
- `public/session/format-loader.ts` — artifact choice, blob evaluation, drop order.
- `tools/build-frontend.mjs` — Trunk, then the three format artifacts. The release half of the build.
- `tools/test-format-loader.mjs` — the pure decisions, in the web lane.

Changed:

- `Cargo.toml` — `[lib] crate-type = ["rlib"]`, the three features, the three members. Default features stay empty.
- `Cargo.lock` — the three path packages. No new registry crates.
- `index.html` — `data-bin="mareader"` on the rust link.
- `src-tauri/tauri.conf.json` — `beforeBuildCommand` runs Trunk, then the format script.
- `.github/workflows/ci.yml` — the three crates named in the wasm lane. The web lane runs the loader test.
- `.github/workflows/release.yml` — `wasm-bindgen-cli` 0.2.127 and `binaryen`, so the script can run inside `beforeBuildCommand`.
- `public/session/boot.ts` — a reader boot does not import pdf.js. `mountFormat` / `dropFormat` on the boot object.
- `src/boot.rs` — a second open on the reader page remounts. Close still leaves the page. `IN_FORMAT` makes `should_swap` false for the format instance.
- `src/features/reader/page.rs` — chrome and an empty slot. No `Viewer`. No document effects.
- `src/components/mod.rs` — `viewer`, `formats`, and the gloss half of `ai` are format features. `ai::settings` stays; the settings modal is host chrome.
- `src/app/effects.rs` — the reader session does not install the document effects. The format mount does.
- `src/components/shell/sidebar/panels/thumbnails/panel.rs` — the host renders `#thumb-scroll` empty. The PDF mount fills it.
- `src/components/shell/titlebar/floating_document_title.rs` — the page rect comes from the snapshot, not from a page-host id.
- `crates/pdf-engine/src/api/search.rs` and `document.rs` — retention deleted.
- `docs/split-wasm-modules.md` — one pointer at this file.
- `tools/bundle-engine.mjs` — emit the loader module for the web-lane test.

## What this phase does not change

The session query stays `/?session=reader`. The path `/reader` stays forbidden. Close still `location.replace`s to `/`. The shelf still does not load pdf.js at boot. `leptos_router` stays in the manifest, unused. Reload Window stays in both menus. There is no slot grid, no hover suggestion, no cover-generation session, and no `memory.discard`. The host is not re-inited in-page. A cached `WebAssembly.Module` is not an instance.
