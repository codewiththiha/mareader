# Roadmap

**Status:** Core reader, library, study tools, neural read-aloud, installers
(`v0.0.7`), and the engine-first architecture (ADR 0007) are complete.
Cross-device library sync (`docs/sync.md`) remains optional future work.
**Parity:** 125 of 125 features (100 %; 86 command-backed, 39 `ui_only`,
2 dropped). Run `cargo xtask parity` for live counts.

## Shipped capabilities

| Area | What it delivers | Details |
|---|---|---|
| Foundation & gates | Layered Rust workspace (`L0`–`L7`), appearance model and tokens, split-pane tree, frameless platform chrome, headless screenshot & kittest harnesses, CI & `xtask` gates | [`docs/architecture.md`](docs/architecture.md), [`docs/design.md`](docs/design.md) |
| Shell & chrome | Docked/floating sidebar rail, appearance popover, unified settings modal, typed settings schema, tooltips, stacked corner toasts, overflow menu, frosted glass bars | [`docs/design.md`](docs/design.md) |
| PDF viewing | Off-thread hayro rendering, shared document pool, vertical/horizontal/single/spread layouts, zoom presets & fit modes, high-zoom tiling, text selection, links, outlines, thumbnails, smooth key/wheel glide | [`docs/pdf.md`](docs/pdf.md), [`docs/thumbnails.md`](docs/thumbnails.md) |
| Appearance & GPU passes | Single wgpu page pass (filter chain, colour blends, seamless pane paper textures, blend backdrop, film grain), saved presets, independent per-pane themes and text | [`docs/adr/0003-pages-are-textures.md`](docs/adr/0003-pages-are-textures.md) |
| EPUB & text pipeline | `rbook` + HTML5/CSS subset + `parley` + `vello_cpu` pagination, book CSS/layout support, vertical CJK/RTL, MathML, footnotes, hostile-input guards, progressive opening, in-memory Markdown & plain-text conversion | [`docs/epub.md`](docs/epub.md), [`docs/epub-layout.md`](docs/epub-layout.md), [`docs/epub-support.md`](docs/epub-support.md) |
| Library | SQLite sync-ready store, read-in-place folders with move tracking, store-copy imports, nested shelves & breadcrumbs, arranged spring-physics grid, multi-selection stacking, fast search, sidebar library panel | [`docs/library.md`](docs/library.md) |
| Study tools & AI | Selection bar, full/half/underline highlights & notes, AI dictionary & page AI (summarize/translate/simplify) over Apple Intelligence or OpenAI-compatible servers, offline dictionary packs, CEFR shading & word pronunciation, proactive/reactive study mode with batch gates and recall | [`docs/study.md`](docs/study.md), [`docs/assist.md`](docs/assist.md), [`docs/page-ai.md`](docs/page-ai.md) |
| Read aloud | System voices plus offline candle neural models (Kokoro in 6 languages, Piper, Kitten, Phonon), word-by-word tracking, floating/tucked player, idle model unloading | [`docs/architecture.md`](docs/architecture.md) |
| Search & keyboard | Streaming cross-line/diacritic-folded document search, full Tab/keyboard focus navigation, auto-scroll, distraction-free mode | [`docs/search.md`](docs/search.md) |
| Packaging & releases | Native installers for macOS (`.dmg` Apple Silicon & Intel), Windows (x64 NSIS & MSI, ARM64 NSIS), and Linux (x86_64 & ARM64 `.deb`, AppImage, multi-arch Arch `PKGBUILD`) verified end-to-end in CI | [`docs/packaging.md`](docs/packaging.md), [`docs/releases/`](docs/releases/) |
| Engine-first & headless CLI | `mareader-engine` (L5) owns all durable state, registered commands (`SPECS`), panic guard, coalescing session journal/replay, `command.batch`, `Host` seam and atomic JSON persistence; `ui-egui` (L6) draws and provides the `Mod+K` command palette; `mareader-cli` (L7) runs scripts with zero toolkit dependencies | [`docs/adr/0007-engine-first.md`](docs/adr/0007-engine-first.md) |
| Live UI control & MCP | Loopback TCP `--control` channel in `apps/mareader` + `crates/ui-egui` (state inspection, command execution, synthetic input injection, PNG screenshots), synchronous stdio `mareader-mcp` server (L6) via `mareader-cli mcp` / `mareader-cli ui`, and `xtask ui-commands` + `xtask panel-commands` gates | [`docs/architecture.md`](docs/architecture.md), [`docs/development.md`](docs/development.md) |

## Optional future work

| Area | Scope | Details |
|---|---|---|
| Library sync (optional) | Multi-device library sync over synced folders, WebDAV, S3, Google Drive or OneDrive using hybrid-clock merge and on-demand files/covers. Sync-ready ids, stamps, tombstones, sort keys and `Library::absorb` are already in place | [`docs/sync.md`](docs/sync.md) |

## Trying and checking builds

- **Desktop:** `cargo run -p mareader`
- **Headless CLI:** `cargo run -p mareader-cli -- commands` (or `exec`, `run`,
  `inspect`); end-to-end scripts live in `apps/mareader-cli/scripts/`.
- **Headless screenshots:** `cargo run -p mareader-ui-egui --example shot -- out.png`
  (keys in [`docs/development.md`](docs/development.md)).
- **Browser preview (best effort):** `tools/web.sh --serve 8080` (supports URL
  state options such as `?preset=sepia&theme=dark&panes=2`).

## Architecture & release history

Detailed per-release notes live in [`docs/releases/`](docs/releases/). Major
post-release and architectural milestones:

| Date | Milestone |
|---|---|
| 2026-11-15 | **Release `v0.0.7`:** unified lookup and AI word cards with CEFR badges and in-place tabs, searchable AI model picker with default provider URL toggle, animated checkboxes, streamlined Vocabulary shading, live GPU theme/blend fades with background saving, flicker-free Read Aloud voice switching, bottom-only read-aloud parking, and static window control icons ([`docs/releases/v0.0.7.md`](docs/releases/v0.0.7.md)) |
| 2026-11-15 | **Release `v0.0.6`:** native ARM64 Windows and ARM64 Linux installers, built-in updater with silent Windows/AppImage updates, progressive EPUB opening and instant reflow, Dial deck Appearance popover, animated icons, command palette, EPUB CSS/picture theming, eight offline dictionary packs, and engine-first headless CLI / MCP control ([`docs/releases/v0.0.6.md`](docs/releases/v0.0.6.md)) |
| 2026-11-15 | **Performance and memory pass:** streamed EPUB cover extraction, cached PDF `RenderSession` across tiles, streamed Kokoro ONNX weights from disk, zero-allocation soft-hyphen search scanning, flattened G2P neural vectors with KV caching, and reused GPU buffers with cached notes rows ([`docs/memory.md`](docs/memory.md)) |
| 2026-11-15 | **Post-split cleanup:** retired transitional re-exports and dead shims, deduplicated presets/menus/sinks, tightened `Session` boundaries, decomposed `read_aloud`/`dict`/`pdf_view` helpers, and fixed `DocumentsView::hit_box` text-layer loading and card anchor rects |
| 2026-11-15 | **Engine-first architecture & headless CLI (ADR 0007):** separated `crates/engine` (L5) from `crates/ui-egui` (L6) behind `Session::execute`, `SPECS`, `Event`, `Host`, and atomic `settings.json`/`session.json` persistence with one-time v0.0.5 import; added zero-toolkit `apps/mareader-cli` with five CI scripts; enforced `xtask wasm`, `xtask trapped`, and command-linked `xtask parity` |
| 2026-11-11 | **Quill animated icon set & generated resting glyphs:** ported full animated Lucide/Quill geometry behind `crates/animated-icons` with tightly cropped neutral-white resting textures (`4 MiB` cache) and active part atlases (`8 MiB` cache), removing `egui-lucide` (ADR 0004) |
| 2026-11-10 | **Workspace hygiene & shared helpers:** added file/function size ratchets (`tools/shape_check.py --dups`), comment/commit checks, shared `crates/nn-core`, `tools/parity-kit`, shared test drivers, and decomposed oversized app, PDF view, EPUB XHTML, and reflow modules |
| 2026-10-31 | **Progressive EPUB opening & player/dictionary layout:** opened large EPUBs around the saved reading anchor before full spine parse completes, and coordinated edge-tucked player and dictionary overlap |
| 2026-10-29 | **Release `v0.0.5`:** word-by-word read-aloud tracking, redesigned player and dictionary, Phonon and Kokoro Tiny voice models, faster neural inference, public-domain Piper voices, pane-tracking zoom ([`docs/releases/v0.0.5.md`](docs/releases/v0.0.5.md)) |
| 2026-10-25 | **Release `v0.0.4`:** offline neural and system read-aloud (Kokoro, Piper, Kitten), word pronunciation, live file reload, book CSS layout, two-tab AI settings, unified modals, stacked toasts, lighter EPUB memory ([`docs/releases/v0.0.4.md`](docs/releases/v0.0.4.md)) |
| 2026-10-20 | **Release `v0.0.3`:** Page AI (summarize, translate, simplify) with spliced EPUB pages, AI library organizer, sidebar library management, named AI models ([`docs/releases/v0.0.3.md`](docs/releases/v0.0.3.md)) |
| 2026-10-18 | **Release `v0.0.2`:** Mareader app icon, notes, bookmarks, highlight export & restyling, redesigned home-screen library, Appearance popover, shortcut sheet, floating dictionary ([`docs/releases/v0.0.2.md`](docs/releases/v0.0.2.md)) |
| 2026-10-17 | **Release `v0.0.1`:** initial cross-platform installers for macOS, Windows, and Linux covering core PDF/EPUB reading, appearance passes, library, study tools, search, keyboard navigation, and audited memory budgets ([`docs/releases/v0.0.1.md`](docs/releases/v0.0.1.md)) |
| 2026-10-13 | **Foundation & functional parity (ADR 0001–0006):** established native egui + wgpu single-process workspace, functional parity ledger, shared multi-pane documents/caches, GPU page texture pipeline, and parley-based EPUB design |
