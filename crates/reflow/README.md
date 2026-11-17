# mareader-reflow

Toolkit-agnostic pure-Rust book typography, pagination, and CPU rasterization engine built on `parley` and `vello_cpu`.

## Capabilities

- Lays out structured document blocks (`Block`, `Role`, `Emphasis`, tables, floats, multi-column sections, inline/block images, SVG) and paginates them into fixed or pane-sized pages (`Flow`, `Typography`, `Design`).
- Supports hyphenation (`hypher`), complex scripts (`parley`), RTL text, and vertical CJK page orientation (`Flow::turned`).
- Rasterizes any page or tile region to premultiplied RGBA8 pixels without panicking (`Flow::render` -> `Option<Pixels>`).
- Tracks reading anchors across reflows (`Flow::anchor`, `Flow::locate`), extracts page links (`Flow::links`), and releases cached per-page layout data on demand (`Flow::rest`).
- Has no GUI or GPU dependencies.

## Dependency

```toml
[dependencies]
mareader-reflow = { git = "https://github.com/codewiththiha/mareader", package = "mareader-reflow" }
```

Requires Rust edition 2024 (`rust-version` pinned in the workspace `Cargo.toml`).

## Usage

```rust
use std::sync::Arc;
use mareader_reflow::{Block, Flow, Fonts, Role, Typography};

let blocks = Arc::new(vec![
    Block::text(Role::Heading(1), "Chapter 1"),
    Block::text(Role::Paragraph, "Reflowable text paginated and rendered on CPU."),
]);

let typo = Typography::default();
let flow = Flow::new(blocks, &typo, Fonts::bundled());

let (w, h) = flow.page_size();
let scale = 2.0;
let region = [0, 0, (w * scale).ceil() as u32, (h * scale).ceil() as u32];
if let Some(pixels) = flow.render(0, scale, region) {
    // `pixels.rgba` contains premultiplied RGBA8 rows (`pixels.width` x `pixels.height`).
    let _ = (pixels.width, pixels.height, pixels.rgba.len());
}
```

## License

MIT. See [LICENSE](../../LICENSE). Bundled reading font notices are listed in [ATTRIBUTION.toml](../../ATTRIBUTION.toml).
