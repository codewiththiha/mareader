# mareader-kokoro

Pure-Rust CPU inference for Kokoro-82M and Kitten neural text-to-speech models built on `candle`, synthesizing 24 kHz mono PCM from phoneme sequences without Python or ONNX Runtime.

## Capabilities

- Loads Kokoro weights from `.pth`, `.safetensors`, `.gguf`, or `.onnx` files alongside `config.json`.
- Loads Kitten `.onnx` models and `.npz` voice packs via `kitten::read_voices`.
- Splits long phoneme inputs into bounded chunks (`chunks`, `chunks_led`) with configurable speaking rate (`SPEEDS`: `0.1..=5.0`).
- Returns per-phoneme frame durations alongside audio (`render_timed`) for word-level synchronization.
- Streams chunk audio incrementally through a callback (`speak`).

## Dependency

```toml
[dependencies]
mareader-kokoro = { git = "https://github.com/codewiththiha/mareader", package = "mareader-kokoro" }
```

Requires Rust edition 2024 (`rust-version` pinned in the workspace `Cargo.toml`).

## Required model files

- Kokoro folder layout: `config.json`, weights (`kokoro-v1_0.pth` or a `.safetensors`/`.gguf`/`.onnx` export), and one or more voice packs (`voices/<id>.pt`).
- Kitten layout: model `.onnx` file and `voices.npz`.

## Usage

### Kokoro

```rust
use std::path::Path;
use mareader_kokoro::{Kokoro, SAMPLE_RATE, Voice};

let model = Kokoro::load(Path::new("/path/to/kokoro"))?;
let voice = Voice::load(Path::new("/path/to/kokoro/voices/af_heart.pt"))?;

let phonemes = model.normalise("həˈloʊ wɜːld.");
let mut pcm = Vec::new();
model.speak(&phonemes, &voice, 1.0, |chunk| {
    pcm.extend_from_slice(&chunk);
    true
})?;
// `pcm` holds mono f32 samples at SAMPLE_RATE (24_000 Hz).
# let _ = SAMPLE_RATE;
# Ok::<_, mareader_kokoro::Error>(())
```

### Kitten

```rust
use std::path::Path;
use mareader_kokoro::{Kokoro, kitten};

let model = Kokoro::from_onnx(Path::new("/path/to/kitten.onnx"))?;
let voices = kitten::read_voices(Path::new("/path/to/voices.npz"))?;
let (_, voice) = &voices[0];

let ids = kitten::ids("həˈloʊ wɜːld.");
let style = kitten::style(voice, "Hello world.")?;
let trace = model.trace(&ids, &style, 1.0)?;
let audio = &trace.audio[..trace.audio.len().saturating_sub(kitten::TRIM)];
# let _ = audio;
# Ok::<_, mareader_kokoro::Error>(())
```

Pair with [`mareader-g2p`](../g2p/README.md) (English), [`mareader-ipa`](../ipa/README.md) (Spanish, Italian, Portuguese, French, Hindi), or [`mareader-hanzi`](../hanzi/README.md) (Mandarin) for end-to-end text-to-speech.

## License

MIT. See [LICENSE](../../LICENSE).
