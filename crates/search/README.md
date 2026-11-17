# mareader-search

Zero-dependency, pure-Rust document search crate providing Unicode/diacritic folding, soft-hyphen-aware cross-line matching, snippet extraction, and hit navigation.

## Capabilities

- Normalizes case, diacritics, ligatures, and punctuation (`fold`, `spell`) while skipping invisible characters and soft hyphens across line breaks (`invisible`).
- Indexes per-page glyph streams (`Glyph`, `Page::new`) and maps match spans back to original text-layer character ranges (`Page::find`, `Page::layer_range`, `hits`).
- Extracts bounded context snippets around matches (`Page::snippet`, `SNIPPET_RADIUS`).
- Provides navigation helpers for stepping across results and scrolling hits into view (`step`, `from_page`, `reveal`).
- Has no external dependencies (`Cargo.toml` `[dependencies]` is empty).

## Dependency

```toml
[dependencies]
mareader-search = { git = "https://github.com/codewiththiha/mareader", package = "mareader-search" }
```

Requires Rust edition 2024.

## Usage

```rust
use mareader_search::{Glyph, Needle, Page, hits};

let glyphs = "Café au lait".chars().map(|ch| Glyph {
    ch,
    line: 0,
    space_before: false,
});
let page = Page::new(glyphs);

if let Some(needle) = Needle::new("cafe") {
    let matches = hits(0, &page, &needle, 100);
    for hit in matches {
        let _ = (hit.page, hit.chars, hit.snippet);
    }
}
```

## License

MIT. See [LICENSE](../../LICENSE).
