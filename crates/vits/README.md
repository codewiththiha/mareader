# mareader-vits

Pure-Rust CPU inference for Piper VITS neural voice models built on `candle`, reading standard Piper `.onnx` and `.onnx.json` files directly without ONNX Runtime.

## Capabilities

- Parses Piper `.onnx` weights and `.onnx.json` voice configurations (`Config::parse`).
- Supports both single-speaker and multi-speaker models (`Vits::speakers`).
- Converts phoneme strings into padded token IDs (`Config::ids`, `Config::ids_at`) and converts `misaki`-style English phonemes to espeak-style phonemes (`espeak_like`).
- Exposes inference controls (`Scales`: `noise`, `length`, `noise_w`) and per-phoneme timing output (`Vits::spoken`, `Vits::speak_timed`).

## Dependency

```toml
[dependencies]
mareader-vits = { git = "https://github.com/codewiththiha/mareader", package = "mareader-vits" }
```

Requires Rust edition 2024 (`rust-version` pinned in the workspace `Cargo.toml`).

## Required model files

A standard Piper voice pair:

- `<voice>.onnx` — model weights.
- `<voice>.onnx.json` — sample rate, `phoneme_id_map`, speaker map, and default inference scales.

## Usage

```rust
use std::path::Path;
use mareader_vits::{Config, Vits, espeak_like};

let cfg_bytes = std::fs::read("/path/to/en_US-lessac-medium.onnx.json")?;
let config = Config::parse(&cfg_bytes)?;
let model = Vits::load(Path::new("/path/to/en_US-lessac-medium.onnx"))?;

let phonemes = match config.style() {
    Some(style) => espeak_like("həˈloʊ wɜːld.", style),
    None => "həˈloʊ wɜːld.".to_owned(),
};
let ids = config.ids(&phonemes)?;
let pcm = model.speak(&ids, 0, config.inference)?;
let sample_rate = config.audio.sample_rate;
# let _ = (pcm, sample_rate);
# Ok::<_, Box<dyn std::error::Error>>(())
```

Pair with [`mareader-g2p`](../g2p/README.md) or [`mareader-ipa`](../ipa/README.md) to convert raw text into phonemes before calling `Config::ids`.

## License

MIT. See [LICENSE](../../LICENSE).
