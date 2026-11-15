# ADR 0004: generated Lucide geometry behind the icon facade

- **Status:** accepted, 2026-10-13; amended, 2026-11-07
- **Context:** Mareader needs a consistent, openly licensed icon set, semantic
  names in UI code, exact resting-to-animation continuity and an idle path that
  does not prepare complete animation atlases. The first implementation used
  Lucide 1.34.0 through `egui-lucide` 0.1. The animated pack now contains typed
  generated geometry for every stock glyph the UI uses, so retaining a second
  copy of those outlines is unnecessary.
- **Decision:**
  - Code names icons by meaning (`ui_egui::icons::Icon`). Only
    `ui_egui::icons::pack` names a generated glyph.
  - `mareader-animated-icons` remains the toolkit-free owner of build-normalised
    Lucide geometry. Declarative SVG is build and parity-test input only.
  - A stock icon at rest calls `raster_resting`, which evaluates only the
    canonical first sample, composites it once on the 24-unit grid and returns
    tightly cropped neutral-white pixels. The UI caches that texture by pack and
    device-pixel side, paints it as one quad and tints it at draw time.
  - The resting cache is context-local and capped at 4 MiB and 512 textures.
    It is independent of the 8 MiB animated-atlas cache, so an idle icon never
    prepares its complete timeline.
  - Interaction lazily requests the existing generated part atlas. Pending,
    failed, disabled and completed runs use the generated resting glyph.
  - Provider logos, voice-engine marks and the AI mark remain the small custom
    static-SVG path. They are not substitutes for stock glyphs.
  - `egui-lucide` is no longer a runtime or build dependency.
- **Consequences:**
  - Resting and animated stock icons share one authored geometry source and
    cannot drift because two icon libraries chose different outlines.
  - First display rasterises one small typed scene synchronously; subsequent
    uses at the same physical size reuse one texture. No XML parser runs for
    stock icons.
  - Cropping saves texture bytes. UVs extend over the virtual 24-unit canvas,
    whose transparent crop border preserves the original margins while the
    glyph remains one egui image quad.
  - DPI changes create another bounded size entry. Release startup, RSS and GPU
    measurements are still required before claiming a performance magnitude.
  - Lucide's ISC attribution remains because its geometry is still bundled;
    only the `egui-lucide` MIT wrapper has been removed.
