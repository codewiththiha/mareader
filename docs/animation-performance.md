# Animation performance

> **Status:** generated resting and part-atlas implementation; release measurement pending · **Last updated:** 2026-11-07

This document answers two separate questions:

1. Does Mareader's current animated-icon provider use a lot of CPU?
2. How should icon and ordinary UI animation be implemented efficiently in a
   native egui application?

The answer comes from the current Mareader implementation and egui 0.36.2.
It is an architectural assessment, not a measured CPU result. A number such
as “10% CPU” or “massive”
would require the release-build measurements specified below.

## Short answer

Mareader's previous animated-icon path did **avoidable CPU work**, but the
source alone does not prove it used “a lot” of CPU on a particular machine.

Before the generated-atlas replacement, each uncached icon sample:

1. evaluates the pack's timeline;
2. builds a new SVG string;
3. parses it into a new `usvg::Tree`;
4. CPU-rasterises it through resvg/tiny-skia;
5. constructs an RGBA `ColorImage`; and
6. uploads a GPU texture.

A normal 15–18 point icon uses a small canvas, usually 26–62 device pixels at
1–2× scale. A run lasts 0.42–0.84 seconds and targets 25–51 stable 60 Hz
samples. The first run for one pack and device size may therefore do 25–51
parse/raster/upload operations; a warmed run reuses those textures. Several
cold live icons multiply the raster work, although their repaint requests
coalesce into the same egui passes.

Cold SVG production was much heavier than interpolating a number and painting
a few egui shapes. The generated part atlas removes parsing from runtime,
performs bounded cold raster work on one native worker, uploads one texture and
then paints source-ordered mesh cells. Whether the change is noticeable on a
particular machine still requires a release profile.

## Implementation status

The runtime-SVG replacement is implemented across
`crates/animated-icons` and `crates/ui-egui/src/icons/anim.rs`:

- the 104 source SVG/timeline declarations are build-script input only;
- generated typed paths, ordered layers and animated-part ancestry are the
  runtime representation;
- time is quantised to stable 60 Hz samples and repaints target the exact next
  sample or landing deadline;
- tight, deduplicated layer-state variants are packed into one neutral-white
  texture per pack and device-pixel side;
- the UI emits those cells as source-ordered quads in one egui mesh, tinting at
  draw time;
- each authored top-level part is one atomic generated layer, so nested opacity
  is baked exactly without a whole-frame representation;
- every pack and DPI bucket must fit its generated part atlas below 8 MiB;
- native cold generation uses one bounded worker; wasm/threadless generation is
  synchronous; and
- the context-local byte-accounted LRU remains capped at 8 MiB and 512 entries,
  with the generated resting glyph visible while an atlas is pending or unavailable.

The generated renderer and ordered compositor have exhaustive source-parity
coverage at the UI's DPI buckets. Release CPU, RSS, upload and driver
measurements are still required before claiming a performance magnitude.

## Mareader's current paths

### Animated icons

`crates/ui-egui/src/icons/anim.rs::advance` owns one finite run per icon and widget.
It selects a quantised sample and asks the context-local cache for the pack and
device-pixel side. A hit selects that frame's ordered atlas cells and paints
all of their quads through one egui mesh and texture. A miss queues one lazy
atlas build on native targets (or builds synchronously where threads are not
available), keeps the generated resting glyph visible, and requests a repaint when the
result is published.

`crates/animated-icons/build.rs` converts the build-only declarations into typed
Bézier geometry, exact nested scenes and flat source-ordered layers. Runtime
code rasterises transformed layer states directly with tiny-skia, deduplicates
identical states, crops transparent margins, and deterministically packs the
variants. Runtime SVG parsing and string construction are absent.

The resting path is cached independently:

- generated resting glyphs use `raster_resting` over generated typed geometry and a
  context-local 4 MiB/512-texture cache keyed by pack and physical side;
- each miss evaluates only the canonical first sample and uploads one tightly
  cropped neutral-white texture, never the complete animation atlas;
- Mareader's custom static SVGs use stable `bytes://` URIs; and
- `egui-lucide` is no longer a dependency.

The repeated work is confined to active pack frames.

### Ordinary UI animation

`crates/ui-egui/src/motion.rs` turns the application's master motion setting into
`Style::animation_time` and `motion::secs`. Most fades, slides, lifts and morphs
use `Context::animate_bool_with_time` or `animate_value_with_time`, then paint
interpolated egui geometry.

In egui 0.36.2 these helpers keep only small numeric state in
`AnimationManager` and request another repaint while the result has not landed.
They do not parse SVGs or upload textures. They still cause the immediate-mode
UI to lay out and paint another pass, so expensive work elsewhere in the pass
must be cached.

Special cases have their own costs:

- `crossfade.rs` captures and uploads one viewport image, then fades that cached
  image;
- `loading_mark.rs` repaints continuously while visible;
- read-aloud meters and progress request bounded 20–30 Hz updates where that is
  enough;
- film grain schedules only the next grain step and uses the page's GPU pass.

## What egui itself provides

The relevant behavior was checked against tag `0.36.2`, the exact version in
Mareader's `Cargo.lock`.

### Reactive scheduling

`Context::request_repaint()` asks for one immediate follow-up pass. Multiple
calls coalesce. `request_repaint_after` keeps only the earliest deadline and can
be pre-empted by input or another earlier request. An idle reactive eframe app
sleeps.

This means a visible animation necessarily updates the whole immediate-mode
UI, but it does not require a permanent loop. CPU efficiency comes from making
each pass cheap and asking for passes only while a visible value can change.

`Context::repaint_causes()` records caller locations and is the first diagnostic
for an application that will not settle. `eframe::Frame::info().cpu_usage`
reports the previous frame's CPU time, including app logic, egui layout,
tessellation and rendering but excluding the vsync wait.

### Native numeric animation

`animate_bool*` and `animate_value_with_time` are the preferred path for
opacity, translation, scale, colour mixing, panel height and other numeric UI
motion. The value is frame-rate independent and the helper requests another
pass only while it is between endpoints.

The official toggle example allocates one stable rectangle, interpolates one
number, and paints its track and knob. This is the shape ordinary Mareader UI
motion should keep: stable layout plus cheap painter changes.

### Image and SVG caching

The 0.36.2 SVG loader caches `ColorImage`s by stable URI and size hint. The
default texture loader caches the corresponding `TextureHandle`s. If one URI is
seen at several sizes, variants not used in the current or previous pass are
removed.

This is excellent for static SVGs. Giving every animation frame a unique SVG
URI would not solve Mareader's problem: each frame is different, so it would
still be parsed and rasterised once, and an unbounded URI stream would become a
cache problem.

### Animated images

The built-in GIF/WebP widget decodes frames, stores their durations, selects an
already-loaded frame, and schedules the exact deadline at which the next frame
is due. It does not request a fixed high-frequency loop between two identical
frames. That is the right scheduling model for quantised icon frames too.

### FrameCache

`egui::cache::FrameCache` retains the result of an expensive computation while
it is requested in consecutive frames and evicts it after it goes unused. It is
useful for frame-local calculations such as syntax highlighting. A texture
cache with an explicit byte budget and lifecycle is a better fit for icon
frames because animations should survive gaps between runs and because
Mareader's resource rules require a hard bound.

## Representation audit: beyond runtime SVG

The current frame cache makes replay cheap, but its source representation is
still an SVG template with holes. A cold sample constructs XML, asks usvg to
parse it, asks resvg/tiny-skia to rasterise the whole icon, copies straight RGBA
pixels and uploads a texture. The useful replacement boundary is therefore the
whole cold-production path, not just the XML parser.

The comparisons below are architectural expectations, not release-profile
results. They narrow the prototype to measure; they do not establish the size
of the current cost.

### What the pack actually requires

A source audit of all declarations in `animated-icons/src/source.rs` found:

- 104 packs, 269 independently animated parts and 336 path elements;
- 56 packs whose parts only transform, fade or stay still;
- 5 packs that use both path length and path offset;
- 34 packs that change stroke width;
- 198 of 269 parts whose pixel coverage does not otherwise change, leaving 71
  parts that need path/stroke variants; and
- 3,240 whole-icon samples if every pack is expanded at 60 Hz, with 26–41
  samples per pack.

The path source is compact: the 336 `d` attributes contain 1,385 commands and
4,353 numeric operands. Geometry remains build-only typed data rather than a
runtime vector asset bundle. Stroke fidelity and dynamic tessellation remain
the harder questions for a pure-vector alternative.

All current strokes inherit round caps and round joins from the SVG root.
Independent parts may translate, rotate, scale non-uniformly, flip, skew and
fade. The largest authored scale key is 2×. Opacity belongs to the whole part,
so overlapping shapes inside one part must be composited before that opacity is
applied. These details prevent a single transformed whole-icon image or a
naive list of translucent painter shapes from matching every frame.

### Candidate comparison

| Representation | Expected warmed CPU | Memory and uploads | Semantic and visual coverage | DPI and renderer fit | Assessment |
|---|---|---|---|---|---|
| Generated epaint paths | Timeline evaluation plus curve flattening and tessellation each pass | Very little persistent memory; ordinary mesh upload with the rest of egui | Transforms and tint are natural. Reveal, offset, stroke width, round joins/caps and part-level compositing need new code | Resolution independent and standard egui | Best pure-vector experiment, not a drop-in replacement |
| Retained epaint meshes | Very low for transform/opacity-only parts; dynamic topology needs variants | Bounded geometry, no icon textures | Good for 198 stable-coverage parts. The other 71 need retessellation or quantised meshes | Standard egui, but antialias feathering is scale-sensitive | Good second prototype if texture memory becomes the limiting cost |
| Raster part atlas drawn as egui meshes | A few affine transforms, UVs and colours per visible part | One or a few bounded RGBA pages per active size; no warmed-frame upload | Preserves group opacity and round-stroke semantics; hard properties use variants for only 71 parts | Size-bucketed, but uses ordinary egui textures and meshes | **Best practical target** |
| Whole-frame sprite atlas | Lowest: select one UV rectangle | Highest raster memory; one upload while an atlas is prepared | Every authored effect is baked exactly | Size-bucketed and standard egui | Lowest-risk control and per-pack fallback |
| Icon or frame font | Low after glyph rasterisation | Competes with egui text in its shared RGBA font atlas | One glyph cannot transform parts independently; full frames require 3,240 glyphs per size | Font rasterisation handles DPI, but arbitrary part rotation/skew is absent | Reject for animated packs |
| SDF/MSDF plus custom shader | Very low draw cost after preparation | Compact scalable fields and quads | Tint, opacity, transforms and some width changes are natural; path progress needs another parameter field or variants | Excellent scale range, but requires an egui-wgpu callback/pipeline and fallback work | Later specialist option, not the first replacement |

A full-frame set for all packs at only one 62-pixel device size is about
47.5 MiB of raw RGBA; at the 192-pixel cap it is about 455.6 MiB. Compression
helps distribution, not resident GPU texture bytes. This confirms that any
sprite design must remain lazy and byte-capped rather than becoming one eager
application atlas.

### Why plain epaint paths are not yet parity-safe

`PathShape`, cubic and quadratic Bézier shapes make generated vector data
possible without SVG. Epaint can also retain an untextured coloured `Mesh`, and
mesh vertices can be transformed or recoloured before submission. This avoids
XML, CPU image rasterisation and icon texture uploads.

Epaint 0.36.2 nevertheless differs from the pack contract in important ways:

- `PathShape` fill supports convex polygons only;
- its ordinary open stroke has no selectable round cap—the tessellator says a
  circular end would be future work;
- joins are mitered or cut at sharp corners, not SVG round joins;
- adding separate circles to imitate round caps/joins over-blends translucent
  parts instead of applying opacity to their composited result;
- path length and offset require flattening each Bézier, retaining cumulative
  arc lengths and cutting one or more visible intervals; and
- transforming a retained antialiased mesh also scales its one-pixel feather,
  so a reusable mesh needs a size/scale bucket or retessellation.

A generated stroker, or a library such as lyon, can close the cap/join and
triangulation gaps. That is real renderer work and needs all-frame visual tests;
it should not be hidden inside a claim that `PathShape` is an equivalent SVG
replacement.

### Why a font is the wrong cache

A conventional icon font works well for static icons. It does not expose the
parts of a glyph to egui, and egui text does not apply an arbitrary affine
transform to each outline. Encoding every quantised whole frame as a glyph
would instead add 3,240 glyphs for one sampling rate before size and DPI
variants.

Egui's font texture is a context-shared RGBA atlas. It is recreated after its
fill passes the atlas threshold and is not governed by Mareader's 8 MiB icon
budget. Animated frame glyphs would therefore move icon memory into the text
cache, increase atlas pressure and still fail to exploit the 198 parts whose
coverage is stable.

### Recommended representation: generated parts in a raster atlas mesh

The practical design is a hybrid: typed generated vector data is the source,
tiny-skia produces neutral-white **part** coverage off the UI thread, and a
normal egui `Mesh` composites atlas tiles at paint time.

1. Extend the pack generator to emit typed paths, rectangles, circles, lines,
   paint flags, part/static-layer membership and source paint order. SVG remains
   build and parity-test input; no XML enters the runtime library.
2. For the 233 stable-coverage parts, rasterise one cropped tile per device-size
   bucket. Preserve a transparent gutter for linear sampling. Parts that can
   grow are prepared at a resolution sufficient for their audited maximum
   scale, or get a coarse scale bucket.
3. For the 71 path/stroke-changing parts, rasterise deduplicated quantised
   variants. Tiny-skia already represents immutable Bézier paths and supports
   round caps, round joins and stroke dashes, so this can preserve the current
   reveal/offset/width semantics without parsing an SVG document.
4. Pack ready tiles into one or a few context-local pages. Publish worker pixels
   to the UI, admit them under the existing byte budget, then use
   `TextureHandle::set_partial` with a per-pass upload budget.
5. Build textured mesh runs in source paint order, splitting a run whenever its
   atlas page changes. Each part contributes four vertices; apply the same
   pivoted affine order as `moves`, put its opacity and current tint in the
   vertex colour, and select its tile with UVs.
6. Keep an authored top-level part atomic when nested opacity or overlap cannot
   be split safely. Its transformed states remain cropped part cells, not
   whole-frame cells.

For the 56 packs without path/stroke changes, this replaces 1,779 possible
whole-frame cells with 146 reusable part cells, plus any static layer. Across
the 48 harder packs, only 71 parts need frame variants; the other 52 parts stay
reusable. Cropped packing matters more than raw cell counts, and all pages must
still be lazy: an upper bound that gives every hard part one full-size tile for
every frame is 2,192 variant cells before trimming.

The compositor retains the strongest properties of both approaches:
software-rasterised round-stroke coverage and group opacity, ordinary backend-neutral
egui submission, independent-part animation, paint-time tinting and almost no
warmed hot-path work. Its trade-off is raster size bucketing. Rotation, skew
and downsampling can also differ slightly from vector rasterisation, so
oversampling and visual thresholds must be measured rather than assumed.

### Proof-of-concept plan

Build the experiment behind the existing provider, not as a second public icon
API:

1. Emit typed geometry for a representative set: `cpu` (13 parts),
   `folder-open` (skew), `circle-alert` (2× scale), `book-open-text` (four path
   parts), `baseline` (path and stroke width), and `palette` (filled dots and
   part compositing).
2. Produce three comparable paths: current full-frame SVG, whole-frame atlas,
   and part-atlas mesh. Add a direct epaint mesh version of one simple stroke
   pack to measure the best non-raster alternative rather than estimating it.
3. Compare every quantised sample at 1×, 1.25×, 2× and the 192-pixel cap. Record
   differing-pixel count, maximum channel error and a saved diff image; inspect
   round corners, group opacity, reveal endpoints and rotated/upscaled parts.
4. In a release native run, record UI-thread frame time, worker preparation
   time, raster count, texture-upload calls/bytes, mesh vertices and resident
   cache bytes. Test one active icon, the maximum expected overlap and replay.
5. Expand the generator and visual test to all 104 packs only after the six-pack
   spike meets its quality and memory thresholds.

Success means a warmed transform-only run performs no SVG construction, XML
parse, full-icon raster or texture upload; one or a few ordered atlas-page mesh
runs carry the visible parts; cache ownership and bytes remain context-local
and bounded; and any unsupported or unadmitted case still paints the generated
resting glyph.

## Recommended icon architecture

Keep the current public provider, trigger policy, stable layout, generated
resting fallback and finite run semantics. Change only how an active frame is
supplied.

### Phase 0: measure the current path

Add an opt-in native diagnostic, disabled in normal builds, that records:

- total app CPU frame time from `eframe::Frame::info().cpu_usage`;
- time in timeline evaluation, SVG construction, usvg parse, resvg raster and
  texture update separately;
- active icon count and raster/upload count per pass;
- `Context::repaint_causes()` when the app does not settle;
- cache hits, misses, bytes and evictions after a cache exists.

Measure a release build in CI/native jobs and on at least one hardware-GPU
machine. Workloads:

1. idle for ten seconds;
2. one click-owned run;
3. a title-bar hover sweep with the maximum expected overlap;
4. View-menu footer and Zoom runs;
5. Animations off;
6. software rendering or a low-end adapter where available.

Record median, p95 and maximum CPU frame time, process CPU, frames produced,
SVG rasters, uploads and cache bytes. Do not call an optimization successful
from FPS alone: a 60 Hz app can still waste most of a core.

### Phase 1: quantised, context-local frame cache

Use a fixed frame index rather than arbitrary floating time:

```text
FrameKey = (pack id, device-pixel side bucket, frame index)
```

At 60 samples per second, the current timelines need about 25–51 frames. The
index's timestamp is the timestamp passed to `frame_svg`, so cache identity and
rendered content cannot disagree. Frames stay neutral white and are tinted when
painted, so theme, hover and disabled colours do not multiply cache entries.

On a miss, the first implementation uses the existing synchronous raster path.
On a hit, it only selects a cached `TextureHandle`. Replaying a warmed icon then
performs no SVG parse, CPU raster or texture upload.

The cache must:

- be owned by the UI/egui context, never global across renderers;
- have an explicit byte cap and entry cap;
- account for RGBA bytes and texture lifetime;
- evict least-recently-drawn entries first;
- never evict an entry used in the current or previous pass;
- remove a whole obsolete size bucket after DPI/zoom changes settle;
- expose deterministic hit/miss/eviction counters for tests; and
- be explicitly cleared with the UI owner or context.

This is the smallest low-risk change and preserves pixel output. Its limit is
that the first run at a new size still pays every miss on the UI thread.

### Phase 2: one lazy whole-frame atlas per pack and size

Once quantisation is stable, pack the frames of one `(pack, side)` into one
texture and draw the selected frame by UV rectangle. Even the longest current
run fits in a small grid: 51 cells fit in an 8×7 atlas. At a common 62-pixel
cell that is about 496×434 pixels, under 1 MiB RGBA.

Build the atlas away from the UI pass:

1. enqueue only the pack and size that is actually requested;
2. rasterise frames on a bounded worker;
3. publish results with pack, size and cache generation;
4. wake egui when pixels are ready;
5. upload with a per-pass count/byte budget; and
6. reject a stale result after eviction, DPI change or cache teardown.

The first run can use the current per-frame path for missing cells while the
worker warms the atlas, ensuring interaction never waits. Later runs change
only UVs. Frequently visible title-bar packs may be warmed during an already
scheduled idle pass, but all 104 packs must not be generated eagerly.

A single atlas handle also avoids a large number of tiny texture objects. The
cache's byte admission is checked before building; if an unusually large
192-pixel atlas cannot fit, the provider keeps the generated resting glyph or uses the
bounded current-frame path instead of evicting visible atlases back and forth.

This remains the lowest-risk next slice and the visual/performance control for
the representation spike. It should not become an eager all-pack asset: the
representation audit shows why whole frames waste memory for the 198 parts
whose coverage does not change.

### Phase 3: generated part scenes and atlas-mesh composition

Once the worker and atlas lifecycle are proven, replace runtime SVG templates
with the generated typed geometry and part-atlas compositor described above.
Reuse one tile for each stable-coverage part; give only the 32 path/stroke parts
quantised variants. Paint ordered mesh runs grouped by atlas page, applying
current transforms, opacity and tint to each quad.

Treat each authored top-level part as an atomic generated scene. That keeps
nested transforms and opacity isolation inside its cropped tile, so rotation,
skew and overlap do not require full-frame cells. Try retained vector meshes
only if measured texture savings justify a custom round-cap/round-join stroker
and scale-aware antialiasing cache.

### Why not a custom GPU renderer first

A wgpu atlas shader could use alpha-only textures and part transforms, but it
adds backend integration, device-loss handling, multiple viewport ownership,
software-renderer behavior and more test surface. Standard egui images and UVs
already remove parse/raster/upload work from warmed runs. Use a custom pass only
if measurements show texture memory or egui image submission is then the
bottleneck.

## Efficient ordinary UI animation

Mareader's ordinary motion is already mostly on egui's efficient path. The
numeric interpolator is unlikely to be the bottleneck; the risk is uncached
work elsewhere in each full UI pass or a repaint request that never settles.
Optimize those call sites only after repaint-cause and scoped timing evidence.

### Keep layout stable

Allocate the final control rectangle once. Animate paint inside it—opacity,
position, scale, colour, underline width—not the surrounding layout unless the
layout change itself carries meaning. This prevents clipping, text movement and
extra reflow. The icon provider's padded fixed box and the formula-bar target
height are examples of explicit geometry.

### Use egui interpolation and painter primitives

Prefer `animate_bool_with_time`, `animate_value_with_time`, easing, `lerp`, and
`Painter` shapes for fades, lifts, tracks, knobs, glows and simple morphs. They
are native egui, carry reduced-motion behavior through `motion::secs`, and
avoid texture work.

### Schedule what can visibly change

- Smooth movement: `request_repaint()` while it is actually moving, allowing
  eframe/vsync to pace it.
- Deliberately capped motion: `request_repaint_after` at the chosen rate.
- Quantised image frames: request the exact next frame deadline.
- Timers and grace periods: request only the remaining deadline.
- Worker work: no polling loop; the worker calls `request_repaint()` when a
  result enters the channel.

A call from one subsystem repaints the whole egui viewport, so independent
requests coalesce. Increasing each subsystem's requested rate does not create
independent partial frames; it only shortens the global deadline.

### Cull and stop

An animation schedules nothing when its widget is not built or its rectangle is
not visible. Continuous indicators run only while their underlying operation is
active. Minimized, unfocused, occluded and software-rendered cases should
suppress nonessential looping motion or use a lower-power rate where the host
exposes that state. Finite runs remain based on elapsed time, not frame count,
so returning to a window cannot leave a control halfway through its motion. The
system reduced-motion setting and Mareader's Animations switch still land
directly on the end state.

### Cache every expensive dependency

During an active animation, assume the surrounding view may run at display
rate. Therefore:

- virtualise long rows with `ScrollArea::show_rows`;
- key and cache document layout, raster pages, syntax highlighting and galleys;
- send viewport commands only when the value changes;
- avoid file I/O, parsing, image decode and document scans in a view function;
- reuse texture handles when pixels change;
- use atlases for many small images;
- cap expensive misses and uploads per pass; and
- keep stale-but-valid output visible until its replacement arrives.

### Match quality to the visual

A progress waveform is readable at 30 Hz. A caret blink needs only its next
transition deadline. A short positional transition benefits from display-rate
updates. Icon frames may remain at 60 samples per second, but 30 samples should
be evaluated in visual CI as a low-power mode rather than assumed equivalent.

## Acceptance tests for an optimized provider

Behavior and layout:

- one provider still owns generated resting glyphs and animated frames;
- no-run, unmapped, disabled and reduced-motion states draw the generated
  resting glyph;
- one initiating event produces one finite run;
- a mid-run event queues at most one replay;
- the final frame is the generated resting image;
- fixed glyph geometry does not clip or shift layout;
- closing a popup stops that widget from scheduling frames.

Cache and work:

- the first quantised frame has one miss and its replay is a hit;
- two widgets asking for the same pack, side and frame share cached pixels;
- a warmed full run performs zero SVG parses, rasters and uploads;
- a warmed part-atlas run performs no SVG construction or full-icon raster and
  emits only ordered mesh runs for its atlas pages;
- cache bytes never exceed the configured admission bound;
- current/previous-pass entries are not evicted into repaint thrash;
- DPI change makes the correct size without retaining unbounded old sizes;
- worker results after cache generation/owner teardown are dropped;
- an idle settled window has no icon-animation repaint cause.

Visual parity:

- compare current and cached frames at start, end, extrema and segment
  boundaries for every pack;
- before removing SVG for a pack, compare every quantised part-atlas sample with
  the current full-frame renderer;
- inspect 1×, 1.25× and 2× output plus the 192-pixel cap;
- keep the existing nearest-announcement, footer-slot and unclipped-frame
  regressions; and
- use remote native screenshots, not the browser preview, for final evidence.

Performance evidence:

- publish current versus optimized release measurements for all Phase 0
  workloads;
- report UI-thread frame time and raster/upload counts, not only FPS;
- report cache peak bytes and evictions;
- identify renderer, scale, operating system and CPU; and
- record known limits in `docs/memory.md` when a cache budget is introduced.

## Approaches to avoid

- Do not move the same parse/raster/upload loop to a different icon API and call
  it caching.
- Do not create a unique egui image URI for every arbitrary timestamp.
- Do not build all packs, sizes and DPI variants eagerly.
- Do not use an unbounded global texture map.
- Do not let background prefetch occupy every worker or outrank visible work.
- Do not continuously request repaints for an idle timer or completed run.
- Do not replace cheap numeric UI motion with video, GIF or SVG frames.
- Do not assume a single GPU transform can express stroke drawing and
  independent-part animation.
- Do not add a second static icon provider; the generated resting renderer
  remains the one stock resting path.

## Primary references

- [egui 0.36.2 `Context`](https://docs.rs/egui/0.36.2/egui/struct.Context.html)
  for repaint coalescing, deadlines and repaint causes.
- [egui cache module](https://docs.rs/egui/0.36.2/egui/cache/index.html) for
  immediate-mode computation caching and generation eviction.
- [epaint 0.36.2 `PathShape`](https://docs.rs/epaint/0.36.2/epaint/struct.PathShape.html)
  and [`Mesh`](https://docs.rs/epaint/0.36.2/epaint/struct.Mesh.html) for the
  native vector/triangle capabilities and convex-fill limit.
- [tiny-skia 0.11.4 `Stroke`](https://docs.rs/tiny-skia/0.11.4/tiny_skia/struct.Stroke.html)
  for direct round-cap, round-join and dash rasterisation without an SVG tree.

## Decision and implementation

Mareader remains a reactive native egui application. Ordinary UI motion keeps
egui interpolation and painter geometry. Animated icons retain the same
interaction, timing, layout, headroom, tint and generated-resting fallback
contract, while the active path uses generated geometry plus ordered part-atlas
mesh composition.

The ownership boundary is deliberate:

- `mareader-animated-icons` is a toolkit-free L0 crate. Its build script alone
  reads the 104 declarative SVG/timeline packs and emits typed geometry, nested
  scene groups, source-ordered layers and timelines into `OUT_DIR`.
- Layer variants bake the quantised transform, path coverage, width and opacity
  state before tight alpha cropping. Equal states of a layer reuse one atlas
  cell. The deterministic packer admits no texture above 8 MiB.
- `mareader-ui-egui` owns semantic `Icon` → pack-id mapping, trigger policy, lazy
  worker publication, the byte-accounted LRUs and egui mesh construction.
  `raster_resting` evaluates only time zero and supplies one cropped texture per
  requested stock pack and physical side under a separate 4 MiB bound.
  Animation cells are appended in source paint order and share one texture and
  one mesh.
- Each authored top-level part is an atomic generated scene. Nested opacity,
  transforms, path coverage and width changes are baked together in that part's
  cropped variants; no whole-frame atlas representation exists.
- Tests compare every quantised frame of all 104 packs with the original SVG
  renderer at 1×, 1.25×, 2× and the 192 px ceiling, then reconstruct every
  atlas frame from its ordered cells and compare it with the direct generated
  frame. Every one-frame resting crop is likewise reconstructed on its 24-unit
  canvas and compared with the canonical first SVG sample.

Release measurement is still required before claiming a CPU or memory
magnitude. The implementation avoids trading runtime parsing for eager or
unbounded storage and leaves retained-vector or custom-shader work contingent
on measured need.
