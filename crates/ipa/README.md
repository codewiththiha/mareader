# mareader-ipa

Zero-dependency, pure-Rust rule-based text-to-IPA phonemizer producing espeak-ng-compatible IPA and Kokoro phoneme strings without `libespeak-ng` or external model files.

## Supported languages

| Variant | BCP 47 tag | Variety |
| --- | --- | --- |
| `Language::Spanish` | `es` | Castilian |
| `Language::Italian` | `it` | Italian |
| `Language::Portuguese` | `pt` | Brazilian (`pt-br`) |
| `Language::French` | `fr` | Metropolitan (`fr-fr`) |
| `Language::Hindi` | `hi` | Hindi (Devanagari) |

## Dependency

```toml
[dependencies]
mareader-ipa = { git = "https://github.com/codewiththiha/mareader", package = "mareader-ipa" }
```

Has no runtime dependencies (`Cargo.toml` `[dependencies]` is empty). Requires Rust edition 2024.

## Usage

```rust
use mareader_ipa::{Language, kokoro, phonemize};

// Standard espeak-style IPA (with stress marks and `^` affricate ties).
let ipa = phonemize(Language::French, "Bonjour le monde.");

// Kokoro-formatted phoneme alphabet.
let kokoro_phonemes = kokoro(Language::Spanish, "Hola mundo.");

// Resolve language from a Kokoro voice identifier.
let lang = Language::for_kokoro_voice("ff_siwis");
assert_eq!(lang, Some(Language::French));
# let _ = (ipa, kokoro_phonemes);
```

## License

MIT. See [LICENSE](../../LICENSE).
