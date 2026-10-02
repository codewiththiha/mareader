# Route and pane lifetime review

Source comparison: `149cbb47e02d563637fe74d6b91b055b59c6211d` (the direct predecessor) versus `887995d3e88ab0571ca7d0a983d6e5411d00cdf0` (the disposable-route change). This review also records the follow-up compiler-boundary cleanup. Read [architecture.md](architecture.md) and [pane-runtimes.md](pane-runtimes.md) for the current implementation contract.

## What changed, and what did not

The per-document architecture was not reverted. The location of the workspace chrome changed deliberately, because code linked into the persistent Shell WASM cannot be unloaded separately from Shell.

### Before: shared route document, independent document panes

```text
Persistent window / Shell WASM
├─ Shell navigation and settings authority
├─ Library view in a div slot
├─ Reader workspace view in another div slot
│  ├─ Title bar, sidebar, settings and menus
│  ├─ Pane tree, layout, dividers and mirrors
│  ├─ PDF iframe → its own PDF WASM, pdf.js session and worker
│  ├─ PDF iframe → another independent PDF WASM and worker
│  └─ Markdown/TXT iframe → its own reflow WASM
└─ Route views could be hidden/recycled in the same window document
```

In `149cbb4`, `src/app/frame.rs::Driver::new` created a `div`, not a route iframe. `post_offer` called `library_runtime::frame::adopt_in_document` or `reader_runtime::frame::adopt_in_document`. Root `Cargo.toml` linked both route crates. The document children were genuine iframes; removing one really removed that child's realm. Removing the Reader **div** could dispose its reactive state, but could not unload the Reader code/linear-memory high-water mark from Shell's still-live WASM instance.

### Now: independent route document, the same independent document panes

```text
Persistent window / Shell WASM
├─ Navigation, settings/persistence authority and route manager
├─ Library iframe → Library WASM
│  └─ Visible on Library; may wait behind an active Reader
└─ Reader iframe → disposable, format-free Reader-host WASM
   ├─ The same Reader workspace chrome
   │  ├─ Title bar, sidebar, settings and menus
   │  └─ Pane tree, layout, dividers and mirrors
   ├─ PDF iframe → its own PDF WASM, pdf.js session and worker
   ├─ PDF iframe → another independent PDF WASM and worker
   └─ Markdown/TXT iframe → its own reflow WASM
```

Five artifact types ship: Shell, Library, Reader host, PDF and reflow. That does not mean exactly five live instances: each opened PDF/text pane instantiates its own child runtime. Reader host uses `reader.html` and `--no-default-features`; PDF uses `pdf,engine`; reflow uses `reflow,engine`. The PDF engine is absent from Shell, Library, Reader-host and text glue. Shell no longer links either route implementation.

The existing `ReaderHost`, `PaneManager`, `PaneTree`, document-pane contract and `FramePane` factory remain. Between the compared commits, `crates/reader-runtime/src/host/` and the appearance-menu/floating implementations were unchanged; the document frame changes were limited to scoped raster retirement and leaf theme/grain ownership. The follow-up only adjusts stale lifetime comments, feature-gates document-only helpers, makes crate warnings fatal, and adds a browser geometry regression. It does not replace the pane implementation.

## Was the last book left open before?

The suspicion identified a real lifetime distinction, but it is important not to conflate the previous document with an empty warmed Reader.

1. **Closing one pane inside Reader** took that pane out of the tree, disposed its document/session and resources, closed its channel, and removed its iframe. Other panes survived.
2. **Navigating to Library** had a different policy. The previous manager could keep a one-pane Reader session intact for `RECYCLE_DELAY_MS = 1_200`, suspend its work, and take it back intact on a very quick return. Afterward it sent `Dispose`, awaited the document teardown, and sent `Rearm` into the same route slot.
3. **Rearm mounted an empty Reader**, not the old PDF. `start_session` called `host.create_root(None)`, and the pane factory constructed an empty document iframe (reflow for the empty descriptor). `WARM_READER_IDLE_MS = 60_000` bounded idle retention, but continued shelf intent renewed that window. Workspaces that had created more than one pane were not eligible for that single-pane recycle; the heuristic also considered a 320 MiB WASM high-water threshold.
4. **Even after the Reader view was removed**, its implementation was still linked into Shell's persistent WASM. Unmounting Rust UI objects frees allocations for reuse, but does not shrink WASM linear memory or unload part of a live module.

There is direct historical evidence for an empty warmed document runtime: the saved `149cbb4` return replay's Chromium and WebKit shelf-intent cases listed `library:active:g1`, `reader:warm:g3` and `pane:reflow` at both +2 and +70 seconds. Their `panes: 0` field described opened/active document state; it was **not** proof of zero physical document iframes. Those are historical separate-run observations, not a controlled RAM savings benchmark against today's build.

Therefore, it is not justified to say the last PDF/worker was permanently leaked. It was explicitly disposed, except during the bounded intact-session grace/quick-return path. But Reader-related residency was deliberately retained/recreated, and its shared Shell module could not be independently unloaded. The current policy removes both causes instead of merely clearing a document object.

## Current return sequence

1. Shell makes Library incoming, or asks its existing Library frame for fresh visible paint.
2. The outgoing Reader pixels remain until the incoming route reports `Ready` and `Painted`; the window is not reloaded.
3. Reader becomes retiring. Shell sends `Dispose` rather than `Rearm`.
4. Reader's `dispose` calls `ReaderHost::dispose` and `PaneManager::dispose_all`: every live/incoming/retiring child rejects stale work, cancels queues, releases sessions/workers/caches/surfaces/virtualizers, closes channels, and removes its iframe.
5. The Reader host unmounts, completes its runtime teardown and acknowledges disposal. Shell's driver removes the **Reader iframe itself**, closes its offers/channel/listeners/timers, unregisters it, and reclaims its descendant raster scope.
6. No shelf intent recreates Reader. `warm_counterpart_unasked` only schedules Library, and `may_recycle` only permits Library. A later book open constructs a fresh Reader host and fresh document realm(s).

Disposal is graceful and bounded, not an assertion that every byte disappears at the instant the URL changes. The host disposal deadline is 8 seconds; children have their existing 1.5-second forced-removal boundary. In the measured four current returns, all Reader/document frames were gone by the +2-second sample and remained absent through +70 seconds.

## Why opening can still feel instant without Reader prewarming

These are different kinds of reuse:

| Mechanism | Retains an old live Reader/document? | Current policy |
| --- | --- | --- |
| Hidden live Reader / empty Reader WASM prewarm | Yes | Disabled on Library |
| Reader view/realm recycle | Yes | Disabled |
| Speculative fetch of PDF/reflow assets on Library startup | Not necessarily an instance, but does speculative Reader work | Removed from `public/shellBoot.js` |
| Packaged assets already on disk, browser HTTP/module-byte/compiled-code caches | No old document/session required | Browser-controlled; fresh instances may benefit |
| Library kept ready behind Reader | No Reader retained | Allowed |
| Showing outgoing pixels until incoming paint | No permanent retention; only bounded overlap | Retained |

`tools/dev.mjs` builds and serves the optimized release artifacts, not an unoptimized development Reader. A book click starts the Reader host immediately; its document iframe starts through the existing factory. Once assets have been used, their bytes/compiled code can be cached without keeping the old runtime's mutable memory, PDF proxy or worker alive. In the packaged app there is no internet download for those assets. A small format-free host, optimized code and paint-preserving handoffs can make a genuine fresh mount feel fast.

That explains compatibility between fast appearance and fresh realms; it is not a measured cold-versus-warm latency attribution. We did not instrument the user's native machine or prove which cache supplied its particular fast open. Browser code caches also do not promise full process-memory recovery.

## Why the unused warnings could coexist with green CI

There was a real coverage gap, not a workflow relaxation. `git diff 149cbb4..887995d -- .github/workflows/` is empty.

- The existing native Clippy command remains `cargo clippy --workspace --all-targets --exclude mareader-shell --locked -- -D warnings`.
- That default workspace build enables PDF and reflow, so the helpers named in the warning are used. `--all-targets` does not enumerate every feature selection.
- The production Reader-host artifact disables both formats. It previously still compiled `open::enter`, session installation/replacement/abandon helpers, `Retiring::detach`, and initial-zoom consumption even though their callers were format-gated.
- `reader-runtime` lacked a crate-level deny-warnings table. Root package lints are not implicitly inherited by other workspace packages. Its standalone artifact build could emit dead-code warnings and succeed; existing runtime/browser tests could also succeed.

The follow-up gates those APIs/module behind `any(feature = "pdf", feature = "reflow")`—the same condition as their real callers—rather than deleting functionality or adding `allow(dead_code)`. It adds the project's `warnings = deny`/`unsafe_code = deny` lint policy to both route crates, making warnings fatal during standalone artifacts, native/wasm builds and both build profiles. Workflows and existing assertions are not weakened.

Cargo also reports a separate future-incompatibility notice for the transitive `proc-macro-error2` dependency. That is not one of these unused application APIs, is not a Reader lifetime signal, and is not hidden by the follow-up. A green strict Rust lint check is not a claim that Cargo has no dependency notices.

Tauri's `Warn Waiting for your frontend dev server to start on http://localhost:1420/` is another unrelated category: the CLI polls `devUrl` while `beforeDevCommand` builds/verifies the artifact set before starting the server. It can be normal during a fresh build. If it never ends, investigate the frontend command's first actual error or missing port; it is not proof that Reader is left running behind Library.

## Appearance-menu assessment

No targeted popup-placement algorithm was changed or rolled back in `887995d`. The same appearance component and floating adapter are used.

The source did expose a collision condition in the prior placement: both route views rendered toolbar IDs such as `toolbar-row` in the same window document. `app_chrome::floating::position::place_at_anchor` uses `document.get_element_by_id("toolbar-row")` and only applies backdrop-filter coordinate compensation when that returned row contains the clicked anchor. A hidden Library toolbar selected first could make that containment check false for Reader. The titlebar's DOM measurements also use document-wide IDs. Separate Library/Reader documents remove those cross-route collisions and keep menu anchors and their filtered containing blocks in the same document.

This is a source-supported explanation for an improvement, not a reproduced diagnosis of the user's exact earlier visual symptom. The added regression opens Appearance by actual hover/click in the Reader document at 1,400 and 640 pixels, moves into the popup past the 400 ms toolbar hide grace, asserts a single Reader toolbar/no Shell toolbar, checks same-document ownership, 288-pixel width, viewport containment and computed anchor placement, and captures both open menus for visual review. It does not change menu geometry to make the test pass. Browser captures are not a native macOS appearance/traffic-light measurement.

## Memory limits

Final architecture revision `887995d` had all four sampled Chromium/WebKit returns at zero Reader and document frames from +2 through +70 seconds. Chromium hands-off PSS went from 344.7 MiB reading to 148.5 MiB at +70 seconds (fresh Library: 168.4). WebKit went from 832.4 to 773.3 MiB (fresh Library: 470.8); its process footprint did **not** fully recover. Separate continuously-reading neighbor cycles still showed WebKit process retention/growth.

Frame removal and balanced session/worker/lane counters validate the intended application lifetime, not the attribution of every retained browser/native byte. See [memory/audit.md](memory/audit.md). Do not claim flat WebKit memory, immediate/full RAM recovery or quantified before/after architectural savings.
