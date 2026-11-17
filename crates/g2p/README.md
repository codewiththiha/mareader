# mareader-g2p

Pure-Rust English grapheme-to-phoneme pipeline porting `misaki` for American and British English, producing Kokoro phonemes and readable IPA notation without Python or `libespeak-ng`.

## Capabilities

- Supports American and British English (`Dialect::American`, `Dialect::British`).
- Implements spaCy-compatible tokenization, part-of-speech tagging, number verbalization, gold/silver lexicon lookup, morphological rules, and an optional neural out-of-vocabulary fallback.
- Supports `misaki` inline override markup (`[word](/phonemes/)`, `[word](+2)`).
- Exposes whole-string phonemization (`phonemize`), per-token mapping (`spelt`, `tokens`), and human-readable pronunciation helpers (`notation::ipa`, `notation::easy`, `notation::raw`).
- `G2p` is `Send + Sync` once constructed.

## Dependency

```toml
[dependencies]
mareader-g2p = { git = "https://github.com/codewiththiha/mareader", package = "mareader-g2p" }
```

Requires Rust edition 2024 (`rust-version` pinned in the workspace `Cargo.toml`).

## Required data files

`G2p::load(dir, dialect)` reads the shared and dialect-specific assets from `dir`:

- Shared: `spacy-en-tokenizer.json` (`TOKENIZER`), `tagger-en.bin` (`TAGGER`).
- `Dialect::American`: `misaki-us_gold.json`, `misaki-us_silver.json`, `g2p-en-us.safetensors`.
- `Dialect::British`: `misaki-gb_gold.json`, `misaki-gb_silver.json`, `g2p-en-gb.safetensors`.

Use `G2p::from_sources(dialect, Sources { .. })` to load directly from in-memory buffers or omit the neural fallback (`neural: None`).

## Usage

```rust
use std::path::Path;
use mareader_g2p::{Dialect, G2p, notation};

let g2p = G2p::load(Path::new("/path/to/g2p-data"), Dialect::American)?;

let kokoro_phonemes = g2p.phonemize("Read the lead paragraph.");
let tokens = g2p.tokens("Read the lead paragraph.");
let display_ipa = notation::ipa(&kokoro_phonemes, Dialect::American);
# let _ = (tokens, display_ipa);
# Ok::<_, mareader_g2p::Error>(())
```

## License

MIT. See [LICENSE](../../LICENSE).
