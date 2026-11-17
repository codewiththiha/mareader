# mareader-animated-icons

Toolkit-agnostic build-time Lucide SVG compiler generating 104 animated icon packs as tightly cropped resting glyphs and bounded 60 fps multi-part RGBA sprite atlases.

## Capabilities

- Compiles Lucide SVG geometry and keyframe/spring motion specs at build time (`build.rs`) into static Rust data with zero runtime SVG parsing (`tiny-skia` only at runtime).
- Rasterizes neutral-white, tightly cropped resting bitmaps (`raster_resting` -> `RestingGlyph`) independently of animation atlases so idle UI stays low-memory.
- Rasterizes 60 fps (`FRAME_RATE`) multi-part sprite atlases capped at `MAX_ATLAS_BYTES` (8 MiB) per atlas with per-frame cell rectangles and normalized UV coordinates (`raster_atlas` -> `Atlas`, `Atlas::cell`, `Atlas::uv`).
- Renders monochrome alpha geometry ready to be tinted by any GPU or CPU renderer.

## Dependency

```toml
[dependencies]
mareader-animated-icons = { git = "https://github.com/codewiththiha/mareader", package = "mareader-animated-icons" }
```

Requires Rust edition 2024 (`rust-version` pinned in the workspace `Cargo.toml`).

## Usage

```rust
use mareader_animated_icons::{find, raster_atlas, raster_resting};

if let Some(pack) = find("panel-left") {
    // Cropped resting glyph for idle rendering (24 px logical canvas at 2x = 48 px).
    if let Some(glyph) = raster_resting(pack, 48) {
        let _ = (glyph.width, glyph.height, glyph.target_x, glyph.target_y, glyph.pixels);
    }

    // Multi-part 60 fps animation atlas generated on demand.
    if let Some(atlas) = raster_atlas(pack, 48) {
        if let Some(cell) = atlas.cell(0, 0) {
            let uv = atlas.uv(cell);
            let _ = (atlas.width, atlas.height, uv);
        }
    }
}
```

## License

MIT. See [LICENSE](../../LICENSE). Icon geometry is derived from Lucide (ISC); see [ATTRIBUTION.toml](../../ATTRIBUTION.toml).
