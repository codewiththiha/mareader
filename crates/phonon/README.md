# mareader-phonon

Pure-Rust CPU inference for Gradium's Phonon flow-matching neural TTS model and Mimi streaming audio decoder built on `candle`, synthesizing 24 kHz mono PCM directly from English text.

## Capabilities

- Takes raw text directly without an external G2P pipeline (`normalize`, `Phonon::chunks`).
- Loads model weights from GGUF or safetensors checkpoints alongside `config.json` and a tokenizer file.
- Primes speaker conditioning caches from safetensors voice embeddings (`Phonon::voice`, `Phonon::voice_from`).
- Streams decoded audio blocks incrementally (`Phonon::say`) or exposes latent generation and Mimi decoding separately (`Phonon::run`, `Phonon::decoder`, `Phonon::decode`).
- Uses deterministic seeded Gaussian sampling (`Noise::new`) and configurable pacing (`Pace`).

## Dependency

```toml
[dependencies]
mareader-phonon = { git = "https://github.com/codewiththiha/mareader", package = "mareader-phonon" }
```

Requires Rust edition 2024 (`rust-version` pinned in the workspace `Cargo.toml`).

## Required model files

- `config.json` — model dimensions, frame rate, and EOS threshold.
- Weights file in `.gguf` or `.safetensors` format.
- Tokenizer definition file.
- Voice file in `.safetensors` format containing `speaker_wavs` (`[1, 512, frames]`).

## Usage

```rust
use std::path::Path;
use mareader_phonon::{Noise, Pace, Phonon, SAMPLE_RATE, TEMPERATURE};

let model = Phonon::load(
    Path::new("/path/to/config.json"),
    Path::new("/path/to/model.gguf"),
    Path::new("/path/to/tokenizer.json"),
)?;
let voice = model.voice(Path::new("/path/to/voice.safetensors"))?;

let mut noise = Noise::new(TEMPERATURE, 42);
let mut pcm = Vec::new();
for chunk in model.chunks("Hello from Phonon.") {
    model.say(&voice, &chunk, Pace::default(), &mut noise, |block| {
        pcm.extend_from_slice(&block);
        true
    })?;
}
// `pcm` holds mono f32 samples at SAMPLE_RATE (24_000 Hz).
# let _ = SAMPLE_RATE;
# Ok::<_, mareader_phonon::Error>(())
```

## License

MIT. See [LICENSE](../../LICENSE).
