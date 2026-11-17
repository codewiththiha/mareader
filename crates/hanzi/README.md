# mareader-hanzi

Zero-dependency, pure-Rust Mandarin Chinese phonemizer implementing word segmentation, number verbalization, pinyin resolution, and Kokoro IPA conversion.

## Capabilities

- Segments Han text using dictionary and HMM emission tables (`dict.txt`, `prob_emit.py`).
- Resolves character and multi-character phrase pronunciations (`pinyin_dict.json`, `phrases_dict.json`) and converts full-width punctuation and numerals.
- Outputs Kokoro-compatible Mandarin phonemes with tone arrows (`Hanzi::phonemize`) and exposes single-syllable conversion (`syllable_ipa`).
- Has no external runtime dependencies (`Cargo.toml` `[dependencies]` is empty).

## Dependency

```toml
[dependencies]
mareader-hanzi = { git = "https://github.com/codewiththiha/mareader", package = "mareader-hanzi" }
```

Requires Rust edition 2024.

## Required data files

`Hanzi::load(dir)` reads the four files listed in `FILES` from `dir`:

- `pinyin_dict.json`
- `phrases_dict.json`
- `dict.txt`
- `prob_emit.py`

Use `Hanzi::parse(chars, phrases, dict, emit)` to construct directly from in-memory UTF-8 strings.

## Usage

```rust
use std::path::Path;
use mareader_hanzi::{Hanzi, syllable_ipa};

let hanzi = Hanzi::load(Path::new("/path/to/zh-data"))?;
let phonemes = hanzi.phonemize("你好，世界。");
let single = syllable_ipa("hǎo");
# let _ = (phonemes, single);
# Ok::<_, mareader_hanzi::Error>(())
```

## License

MIT. See [LICENSE](../../LICENSE).
