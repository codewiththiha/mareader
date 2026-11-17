# mareader-epub

Pure-Rust EPUB parser, HTML5/CSS subset and MathML layout engine, and paginator built on `rbook` and `mareader-reflow`.

## Capabilities

- Opens EPUB archives with hostile-input limits (`MAX_ENTRIES`, `MAX_MARKUP`, `MAX_DEPTH`, `MAX_PICTURE_BYTES`) either all at once (`Book::open`) or progressively around a saved reading target (`OpenBook::new`, `OpenBook::step`, `OpenBook::step_cancellable`).
- Parses XHTML, book stylesheets, embedded WOFF/WOFF2/obfuscated fonts, MathML, SVG, fixed-layout spreads, media overlays (`Narration`), and WebVTT/TTML subtitles.
- Converts Markdown and plain-text inputs into valid in-memory EPUB packages (`convert`, `Format::Markdown`, `Format::Plain`).
- Renders paginated RGBA8 tiles (`Book::render`), extracts per-page text layers (`Book::text`), resolves internal/external links and tables of contents (`Book::links`, `Book::toc`), and re-paginates while preserving reading anchors (`Book::retypeset`, `Book::anchor`, `Book::locate`).
- Extracts metadata and covers without full pagination (`sniff`, `describe`, `cover`, `describe_file`, `cover_file`).

## Dependency

```toml
[dependencies]
mareader-epub = { git = "https://github.com/codewiththiha/mareader", package = "mareader-epub" }
mareader-reflow = { git = "https://github.com/codewiththiha/mareader", package = "mareader-reflow" }
```

Requires Rust edition 2024 (`rust-version` pinned in the workspace `Cargo.toml`).

## Usage

### Opening and rendering an EPUB

```rust
use std::sync::Arc;
use mareader_epub::Book;
use mareader_reflow::Typography;

let bytes = Arc::new(std::fs::read("book.epub")?);
let typo = Typography::default();
let book = Book::open(bytes, &typo)?;

let (w, h) = book.size(0);
let scale = 1.5;
let region = [0, 0, (w * scale).ceil() as u32, (h * scale).ceil() as u32];
let pixels = book.render(0, scale, region);
let lines = book.text(0);
# let _ = (pixels, lines);
# Ok::<_, Box<dyn std::error::Error>>(())
```

### Converting Markdown to an EPUB buffer

```rust
use mareader_epub::{Format, convert};

let md = b"# Notes\n\nParagraph text.";
let epub_bytes = convert(md, Format::Markdown, Some("Notes"), &|_| None)?;
# let _ = epub_bytes;
# Ok::<_, mareader_epub::OpenError>(())
```

## License

MIT. See [LICENSE](../../LICENSE).
