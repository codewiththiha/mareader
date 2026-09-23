# Separate WASM instances, so a closed book can release its heap

Branch `split-wasm-modules-t5`, cut from `main`. This is the plan. No app code moves until a later pass.

The goal is the one the memory model already names and cannot reach from inside the current binary: when the reader leaves a heavy surface, that surface's WASM heap has to become unreachable, so the browser can collect it. A library session and a reading session must not share a linear memory. Inside a reading session, a PDF pane, a text pane, and a Markdown pane must be able to live together and die one at a time. Split view falls out of that, if the panes are slots in a host rather than routes in one module.

## Decision

Compile separate WASM artifacts. Each gets its own `WebAssembly.Memory`. A JS bootloader instantiates them and drops them. That is the only shipping mechanism that returns a WASM heap to the browser.

Do not code-split the existing binary. Leptos `#[lazy]` / `cargo leptos --split`, and the wasm-bindgen splitter they come from, load extra files into the **same** memory so a lazy view can hold pointers into the parent heap. Leaving the route unloads code. It does not reclaim the heap. This repo is Trunk CSR on Leptos 0.8.3. It does not use cargo-leptos. Adopting that toolchain would be a migration that misses the goal even on a newer Leptos.

Do not depend on `memory.discard` or the Component Model. Both sit in Phase 1 of the WebAssembly proposals list (README as of 10 Aug 2026; Memory control is championed by Deepti Gandluri and Ben Visness). `memory.discard` zeroes a range and may decommit physical pages. It does not shrink `byteLength`, and it does not fix commit limits. Feature-detect it later as a hint. Never as a step.

The shape:

| Instance | Alive | Holds | Must not hold |
|---|---|---|---|
| JS bootloader | For the life of the webview | Which session is up, the loader, the storage bridge, a `WebAssembly.Module` cache | Document bytes, an `Instance` after drop |
| `library.wasm` | While the shelf is showing | Library UI, shelves, covers as stored JPEGs | pdf.js, a parser, a page host, the reader rail |
| `reader-host.wasm` | While a book is open | Title bar, rail geometry, menus, focus, the slot grid | Document bytes, pdf.js, a block tree |
| `pdf.wasm` + pdf.js | While a PDF pane is open | The pdf.js document, canvases, the search index, PDF page hosts | The library graph |
| `text.wasm` | While a text pane is open | That file's blocks, its cut, its page hosts | A PDF document |
| `md.wasm` | While a Markdown pane is open | Same, plus the heading outline | A PDF document |

Library and the reading group never run together. Opening a book drops `library.wasm`. Returning to the shelf drops the host and every format instance. Closing one pane drops that format instance and leaves the others. Split view is several format instances mounted into host-owned slots.

Two corrections to the sketch, both load-bearing:

- The host is part of the reading group, and it dies with the group. A process-lifetime shell would be a heap that never dies, which is the current bug in a smaller box. The chrome is already a large Leptos tree (`app-chrome`, `ShellController`, `AppearanceMenu`). Rewriting it in JavaScript is a rewrite, not a split. It stays WASM, and it is mortal.
- Rust cannot drop its own instance. The thing that actually releases the heap is a few hundred lines of JavaScript that nulls every handle. The bootloader is not a fifth Leptos app.

## What the platform actually does

Sources, checked against the proposals list and the memory-control overview, not against folklore:

- A linear memory is one buffer (`WebAssembly.Memory`, backed by an `ArrayBuffer`). `memory.grow` is monotonic. The shipping spec has no shrink. Freeing a Rust allocation returns the block to the allocator **inside** that buffer. `byteLength` does not go down. A live instance's footprint is its peak. This is also what `src/memory.rs` and Mareader.md already say, and they are right.
- The only way to make that buffer collectable is to drop every JavaScript reference to the `WebAssembly.Instance`, its `Memory`, every typed-array view of `memory.buffer`, and every closure the glue holds, then let GC collect the instance. A cached `WebAssembly.Module` may stay. That is code, not the heap. A cached `Instance` must not. This is the established workaround; there is no instance-destroy instruction.
- Two modules that import one `WebAssembly.Memory` share one heap. Destroying one does not free it while the other lives. The wasm-bindgen split prototype's loader passes `memory: mainExports.memory` into the chunk. That is the design Leptos split is built on. It is the wrong design here.
- Two modules compiled separately, each exporting its own memory, have two heaps. Anything that is not a number is copied through JavaScript. That copy is the price of being able to drop one heap. There is no shared Rust reference, no shared Leptos `Signal`, no shared allocator. Common code (allocator, `fmt`, serde, Leptos) is duplicated per artifact. Accept that. A shared `runtime.wasm` imported by the others stays alive while any importer is alive, and its memory becomes the new ratchet.
- Terminating a worker discards that worker's heap immediately, including a WASM instance that lived only there. Leptos mounts DOM, so the view modules stay on the main thread. pdf.js already runs the document in a worker. `LoadingTask.destroy()`, called from `destroy()` in `public/pdfEngine.ts`, is what kills that worker. The main-thread `pdf.min.mjs` graph stays until something drops the module.
- On WKWebView, the platform Mareader.md measures, a collected buffer may still not show up as a lower Activity Monitor number. Freed pages sit in `MADV_FREE` until pressure. JSC and bmalloc keep their own free lists. Canvas backing stores are GPU memory and are not in the WASM heap. Reload Window (`src/services/reload.rs`) stays the hard reset. The split removes the structural ratchet. It does not repeal WebKit.

`memory.discard` is not a plan step. If `Memory.prototype.discard` is ever present, call it on the way out as a hint. Absence is a no-op.

## Where the bytes are today

One Trunk target (`index.html`, one `data-trunk rel="rust"`). One `mareader` package. One linear memory for the library, the reader, every format, and the Leptos runtime. Routes are `/` and `/reader` in the same tree (`src/app/shell.rs`). Unmounting `ReaderPage` drops the view. It does not drop the module. `close_document` resets signals and calls `engine::destroy()`. The heap probe exists so a climb per open/close cycle can be told from a latch. The latch is the platform. The single immortal heap is the app's.

The manifest's own note: an unoptimized module was 46 MB and about half of a 400 MB idle footprint; a release module is about 713 KB. The problem is not the code size. It is the one heap that has seen every book.

**Rust heap. This is the ratchet the split removes.**

- The library graph. Books, shelves, folders, and cover JPEG data URLs parsed into signals (`src/storage`, key `mareader.covers.v1`). Alive for the process, including while a book is open.
- A reflowable book's blocks. `read_file_text` pulls the file into a `String`. `txt-core` / `md-core` turn it into `Vec<TextBlock>`. That lives in `ReflowContent` until `DocumentState::reset`. Reset drops the `Vec`. The arena does not shrink. A large Markdown file is a WASM-heap cost, not a canvas cost.
- The PDF search index. `crates/pdf-engine/src/api/search.rs` keeps it across close on purpose, keyed by the pdf.js fingerprint, because rebuilding it on every open ratcheted this same heap. Correct for one immortal instance. Pointless once the instance dies.
- Outlines, gloss caches, the reactive graph of whichever page is mounted. Small beside a book, and they live in the same buffer, so they pin the peak.

**Not the Rust heap. A Rust split does not free these by itself.**

- PDF page pixels. pdf.js 6.2.108 is vendored under `public/vendor/pdfjs`. It is not an npm dependency. The engine is JavaScript (`public/pdfEngine.ts`, `public/engine/*`). Canvases dominate the OS number. Mareader.md already says so. `destroy()` already releases page surfaces, clears the thumbnail cache, and destroys the loading task.
- pdf.js itself, loaded unconditionally in `index.html` before the WASM (`pdf.min.mjs`, then `pdfEngine.js`, then the rust link). The comment in that file explains the race: the app probes `window.PDFReader` as soon as components mount. The cost of winning that race is that the shelf pays for the engine when no PDF is open. pdf.js 6 can decode JBIG2, CCITT, and JPEG 2000 through optional WASM modules if a wasm URL is set. This engine does not set that URL, so those decoders are not a current resident cost. If a later bump turns them on, they die with the worker, which is another reason the worker has to actually be destroyed.
- The webview's own high-water mark. Unchanged in kind. What should change is the peak it latches onto, because the library heap and the reader heap stop being the same peak.

## Why the other shapes fail

**One reader WASM that still contains pdf, txt, and md.** Returning to the library would drop it. That is the first win, and it is the right intermediate. It is not the end state. Switching a pane from PDF to text would not drop the PDF heap, and a split view could not drop one pane. The later requirement rules it out as the destination.

**Five always-on Leptos apps.** Library, host, pdf, text, and Markdown instantiated at boot and kept resident. That multiplies peaks. The library must be dead while reading, and a format module must be dead while its pane is closed.

**A shared runtime module.** Covered above. Duplicate `reader-core`, `reflow-core`, `pdf-paper`, `ui-geom`, `virtual-list`, and `app-chrome` into each artifact that needs them. They are already leaves or near-leaves. Disk cost is a few release modules in the current size class, for a desktop app. Resident cost is one session. `app-chrome` linked into both the library and the host is fine: they do not run together. The standalone `cargo check -p app-chrome --target wasm32-unknown-unknown` already in CI is what catches a feature the other crate was quietly providing. Keep it.

**Parsing in a worker and calling that the split.** Optional later, and it does not remove the block tree from the view instance. The view instance is the one that has to be droppable. Do not block the plan on it.

## Architecture

```
WKWebView
└── JS bootloader                    always, tiny, no document bytes
    ├── library.wasm                 XOR
    └── reading session
        ├── reader-host.wasm         chrome, slots, focus
        └── slots
            ├── pdf.wasm + pdf.js worker
            ├── text.wasm
            └── md.wasm
```

The bootloader is the only owner of instance handles. A Leptos component must not close over the loader. If it does, the drop is theatre.

**Routes.** Two document lifetimes, owned by the bootloader, not by `leptos_router` inside one binary. `/` is the library session. The reading session is `/?session=reader` — a query on the entry URL, not the path `/reader`. Tauri's asset protocol does not SPA-fallback, so a full GET of `/reader` can 404. The Phase 1 spec is `docs/phase-1-session-boundary.md`: the drop is a navigation of the existing Trunk binary, not a second wasm file and not an in-page instance swap. Panes are not routes. A route per format cannot be a split view, and a route change that tore the host down would kill the other pane.

**Slots.** The host renders a grid of empty divs. A format module calls `mount_to` on the div it is given, never `mount_to_body`. The host does not import the format crate. It does not know a PDF page host from a reflow page host. It knows a slot id, a format, and a bridge. `UniversalPageHost` stays inside the format module. The viewer's rule — a layout may not name a format — survives, one level up: the host may not name a format's widgets, only its module.

**The wire is copies, because the heaps are not shared.** The types already exist and are already serde:

- `Appearance`, persisted inside `mareader.settings.v1`. Field names are a contract. Do not rename them.
- `ReadPoint`, book id, path. `library-core`.
- `OutlineNode`. `reader-core`. PDF outline entries are already converted to this before the panel sees them (`DocumentState::set_pdf_outline`).
- Paper color. `pdf-paper`'s palette, already published as `--pdf-paper` by the engine.

Down: open payload, zoom, go to page, search, an appearance patch, "the menu is open" (the PDF raw-retention gate in `AppearanceMenu`), slot size, blend on or off.
Up: title, page, page count, outline, paper color, a selection summary.

Numbers and short strings can be function arguments. Anything else is one JSON envelope through one exported function on each side. Not a new glue crate per call. The DOM contract (`src/dom_contract.rs`, `public/engine/dom-contract.ts`, gated by `check-dom-contract.js`) stays inside the format module that owns the page. The host does not grow a second copy of the `sp-` / `dp-` / `hp-` / `cont-` ids.

**CSS crosses the boundary for free.** Custom properties inherit into the mounted subtree. The text page already paints `var(--tx-paper)`. Blend mode does not require the text module to know what a PDF is. The host sets the variable on the slot. The PDF module publishes its paper upward; the host writes the variable. That is the whole blend path.

**Storage is the bus between sessions.** It already is, almost. Settings, the library blob, and covers are localStorage, loaded synchronously at boot because a late theme flashes (`src/app/bootstrap.rs`). After the split, `reading_progress` cannot write `state.library.books` in-process. That signal's owner is dead. It writes the resume point through the same blob the library will load next time. Gloss marks already key by row id (`gloss_key`) and already persist. The open payload carries the marks for that row. A stroke writes back through the bridge, and teardown flushes before the instance is dropped. That is the same obligation `flush_read_point` already has for close and for reload.

**Covers do not link the library to pdf-engine.** They are stored JPEGs. Generating one is a transient PDF session at import: the bootloader brings up pdf.js and `pdf.wasm`, renders page 1 through the existing `cover_data_url` path, writes the JPEG, drops both. The library instance only displays what storage already has. A shelf with a missing cover asks the bootloader. It does not import the engine.

**Search-index retention goes away with the PDF instance.** Re-extraction on the next open grows a new heap that dies on the next close. That is the trade the retention was invented to avoid, and it is the right trade once the heap is mortal. Do not persist the index unless a measured reopen is actually slow. Persisting it into the host would put the book back into the long-lived heap.

**The open race moves to the bootloader.** `session::claim` exists because an open's tail outlives the click, and because `leptos::task::spawn_local` is cancelled when the card unmounts — the open uses `wasm_bindgen_futures::spawn_local` for that reason (`src/services/document/open/mod.rs`). A future spawned inside the library instance dies with that instance. Sequence: the library asks the bootloader to open a path; the bootloader bumps a generation, flushes, drops the library instance, instantiates the host and the format module, and hands them the open payload. The format module does the file read or the pdf.js open. A second open bumps the generation and the loser stands down, the same rule as `session::owns`. The bootloader owns the generation. The dying instance must not.

**pdf.js leaves the boot HTML.** After the split, the PDF module's glue dynamic-imports the engine and only then mounts. The host does not probe `window.PDFReader`. The engine smoke test stays. It tests the engine, not the boot order. Worker URL resolution in `public/engine/loader.ts` already special-cases Tauri. The dynamic import has to keep that, and it has to use a relative URL so the asset protocol and `trunk serve` both resolve `pdf.worker.min.mjs`.

## Focus, sidebar, appearance, split view

`ShellController` keeps owning rail geometry: docked versus overlay, the slide, the traffic-light gutter, `ChromeSurface`. Those are window facts. `ChromeSurface::Reader` versus `ChromeSurface::Library` stays the session distinction. What fills the rail is a question about the focused pane, asked over the bridge.

- Focused PDF: outline from the module, thumbnails rendered by the PDF module into a div the host provides. The canvases live in the PDF instance, so closing the pane drops them. The two-deep render lane, the 12M pixel ceiling, and the 16-entry thumbnail LRU stay where they are. They are what keep a **live** instance's peak down. The split does not replace them.
- Focused Markdown: outline from headings, no thumbnail panel. Headings are already in the text (`md-core`), so there is no resolver tail.
- Focused text: no outline, no thumbnails. The rail can show document info only, which the sidebar already knows how to do.

The appearance menu stays on the title bar, in whichever session is alive. The library's menu is inside `library.wasm` (no texture section, as today). The reader's menu is inside the host. Which sections show is already a surface question: texture only when the surface is the reader and the document is raster (`AppearanceMenu`, `texture_applies`). That becomes "the focused pane is PDF". Mode, tint, presets, and grain stay the window's, and they are written to `mareader.settings.v1` so the other session sees them on its next boot. There is no live sync, because the two sessions are never both alive. Texture, the paper rebake, and the "menu is open, keep the unbaked raw" flag are sent to the focused PDF module. A text pane does not rebake rasters. It repaints CSS tokens from `Appearance`, which it already does.

Blend, locked so it does not get re-decided later:

- Blend on, and a PDF pane is open. That pane's paper is the group paper. The host writes it onto text and Markdown slots. The PDF pane is unchanged. It already is that paper. This is the "main look respects the PDF" rule, and it is a CSS variable, not a shared theme engine.
- Blend on, no PDF pane. Nothing to extract. Each text pane uses the theme paper. Do not invent a paper color.
- Blend off. Each pane keeps its own look. The menu edits the focused pane. A text pane's paper is the reflowable palette. A lone PDF pane keeps today's `blend_backdrop` behavior: its own detected page paper, driven by scroll position inside that module. The group rule above is only the extra case split view adds.

The color is a few numbers. The modules do not need to share a heap to share a color. The persisted schema does not change.

**Split view is a field, not a route.** The host grid starts at one slot, which is today's reader. An empty slot, or the edge of a filled one, shows a suggestion on hover. The choices are the formats the registry already lists (`reader-core` `SUPPORTED`: PDF, Text, Markdown). Dropping a file on a slot opens that format there. Clicking the suggestion opens the dialog scoped to that slot. Two panes are two instances. Closing one drops that instance and leaves the others. The host does not restart.

This works with separate memories because a pane does not need another pane's pointers. It needs a rectangle, a focus bit, and a handful of copied values. The hover suggestion is host chrome. It is not a module and not a route.

What split view must not do: mount two Leptos apps that both think they own `document.body`, the title bar, or `AppState`. One host owns chrome. A format module owns its slot and nothing above it. Gloss and the selection pill stay inside the format module, because they walk that format's DOM, and they portal to `document.body` if they have to escape the slot's overflow. A portaled node is still DOM owned by that instance's closures. Teardown removes it before the instance is dropped, or the closure pins the heap. The repo has already had this class of leak (a link layer that outlived its host, called out in Mareader.md). The drop protocol is the rule that prevents the sequel.

Zoom, search, and page commands go to the focused pane. The host does not virtualize pages and does not own a zoom scale. The zoom controller stays inside the format module, next to the virtualizer it already drives.

## Drop protocol

Order is the feature. Getting it wrong leaks the instance through a closure, and the split did nothing.

1. Flush. Resume point, an in-flight gloss stroke, any settings dirty bit. Same duty as close and reload today.
2. Dispose the Leptos root owner for that mount. Cleanup runs here. ResizeObservers disconnect. The appearance-menu flag clears. Listeners drop. A cleanup that runs after the instance is gone aborts the runtime. The README already calls this out for resize observers.
3. Format teardown. PDF: the existing `destroy()` — sweep, release canvases, destroy the loading task, null the document. Then drop the dynamic import of pdf.js so the module graph is unreachable. Text and Markdown: disposing the owner drops the block `Vec`. There is no second engine.
4. Null the glue. Every export, the wasm-bindgen object, every `Uint8Array` over `memory.buffer`. A view keeps the `ArrayBuffer` alive. A view taken before a `memory.grow` is detached; do not cache views on the host.
5. Drop the `Instance`. The `Module` may stay in a small cache keyed by format, so the next open skips download and compile. Do not keep the instance.
6. The bootloader's handle map is the checklist. If a key is still set, the drop is not done.

Log `[mem]` from inside the instance at mount and at the start of teardown, and log from the bootloader once the handle is null that the instance is gone. Read the shape, not the OS number, which is the rule `src/memory.rs` already states. A new instance starts at its own floor. Five open/close cycles do not climb, because each cycle is a new instance. A climb **inside** one instance is still a leak. Activity Monitor falling is welcome and is not the pass condition.

## What this will not reclaim

- WebKit's physical footprint can stay high after the `ArrayBuffer` is unreachable, until pressure or a webview reload. Reload Window stays in both menus.
- A live instance still never shrinks. A PDF pane left open for an hour, zoomed to the pixel ceiling, keeps that peak until the pane is closed. The render lane, the pixel cap, and the scroll-settle raster policy stay.
- Canvas and GPU memory are released by the engine teardown, not by dropping the Rust instance. Both have to happen. Dropping `pdf.wasm` without `destroy()` leaks the worker.
- The bootloader and a cached `WebAssembly.Module` stay. They are small. Do not put document bytes in either.
- Cover JPEGs in the library instance are a bounded heap (`COVER_CAP`). Acceptable. Do not also keep the PDF instance alive to avoid storing them.

## Build and CI

Trunk builds one rust crate from `index.html`. Extra artifacts are extra `cdylib` crates, built in CI and in the release workflow, run through `wasm-bindgen` and `wasm-opt -z`, and copied into `dist/` beside the trunk output. Tauri's `beforeBuildCommand` is `trunk build --release`. `frontendDist` is `../dist`. The release command grows. The dist directory does not move. URLs are relative, so the asset protocol and `trunk serve` resolve the same files.

This environment does not compile Rust. The wasm check in `.github/workflows/ci.yml` is `cargo check --target wasm32-unknown-unknown` on the root package, plus the standalone `app-chrome` check. Each new bin has to be named in that lane or it rots. The web lane's contract scripts stay the gate that does not need a Rust compiler: formats (`check-formats`, three declarations that cannot see each other), versions, DOM ids, window events, chrome numbers, engine smoke. A bridge that invents a second DOM id or a second event name fails those on purpose. Appearance field names stay. The README test count stays, so a later code change that adds tests updates the README in the same commit.

Do not add a GC assertion to CI. Collection is not deterministic. Assert that the loader's handle map is empty after a drop, in the engine-smoke style: plain Node, stubbed. Measure the heap shape in the webview, with the existing `[mem]` lines, when the session boundary first lands.

## Phases

Overview only. Each one is a later pass. None of them is a shared-memory split, and none of them rewrites Mareader.md's memory model into a promise the platform cannot keep. The model stays: peaks are ours, the floor is the webview's, a reload is the hard reset. The new sentence is that a session's peak dies with the session instead of becoming the next session's floor.

1. **Session boundary.** `library.wasm` and one `reader.wasm` that still contains every format. Bootloader swaps them. pdf.js becomes a dynamic import owned by the reader. Storage bridge for resume, gloss, and covers. Leaving `/reader` drops the reader instance. This is the exit-to-library win, and it ships on its own. The reader binary is allowed to be fat. What it is not allowed to do is stay alive on the shelf.

2. **Format instances.** The fat reader becomes the host plus `pdf.wasm`, `text.wasm`, and `md.wasm`. One slot. Switching format, or closing the book, drops the format instance. Closing the book also drops the host. Sidebar and appearance talk across the bridge. Search-index retention is deleted here, because its reason is gone. The spec is `docs/phase-2-format-instances.md`.

3. **The grid.** More than one slot. Hover suggestion from `SUPPORTED`. Focus moves the rail and the appearance sections. Blend writes the PDF pane's paper onto the other slots as CSS variables. No new route.

4. **Loose ends.** Cover generation as a transient PDF session, so the library crate does not depend on `pdf-engine`. Confirm the worker is gone after `destroy`. Keep Reload Window. Optionally feature-detect `memory.discard` on the way out of a live PDF instance, behind a presence check.

## Answers

**Can leaving the PDF page for the library drop that WASM instance completely?** Yes, if the PDF page is its own instance — or, in the first phase, if the whole reader is — and the bootloader drops every reference in the order above. Unmounting today's `ReaderPage` does not do this. It only drops the view inside the one heap.

**Can txt, md, and pdf each be a module, and the library another, and each be deleted from RAM on the way out?** Yes, as separate instances with separate memories. "Deleted" means unreachable and then collected, not shrunk in place. The library instance and the reading instances must not be alive together, or each pins its own peak and the shelf-after-reading number is the sum.

**Will those modules be able to do split view?** Yes. Separate memories are what make a pane safe to close, not what make split view impossible. They coexist by being mounted into host slots and copying small values. They must not share a Leptos context, and they must not import each other.

**A route, or a field with a hover suggestion?** A field. The only route is the session boundary, because that is the lifetime boundary. A hover suggestion on an empty slot is host chrome, drawn from the format registry.

**Host sidebar acting on focus, reading group inside, library outside?** Yes, with the two corrections at the top. The host dies with the reading group. The library never sees those modules, because it is not running. The sidebar's behavior-on-focus is the host asking the focused module what to show.

**Appearance follows the focused format, and PDF paper color can tint text and Markdown in blend mode?** Yes. The menu already switches sections by surface. Focus replaces "the one open document". Blend is a CSS variable the host writes from the PDF module's paper snapshot. The text module already paints that variable. No shared heap. The settings schema does not change.
