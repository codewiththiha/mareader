<p align="center">
  <img src="apps/mareader/icons/128x128@2x.png" width="104" alt="Mareader app icon">
</p>

<h1 align="center">Mareader</h1>

<p align="center"><strong>A reader for PDFs and EPUBs, shaped around the way you read.</strong></p>

<p align="center">
  Open a quiet page or build a four-pane reading desk. Keep notes, listen as you read,
  and bring in AI when it helps.
</p>

<p align="center">
  <a href="https://github.com/codewiththiha/mareader/releases/latest">Latest release</a>
  · <a href="https://github.com/codewiththiha/mareader/releases">All releases</a>
</p>

## Features

| Feature | What it does | Highlight |
| --- | --- | --- |
| PDF and EPUB | Read, search, follow links, and move through pages, spreads, or continuous scroll. | [Reading spread](docs/screenshots/readme/reader-default.jpg) · [Vertical Chinese EPUB](docs/screenshots/readme/chinese-vertical-epub.jpg) |
| Four-pane workspace | Keep up to four books open side by side to compare text and references. | [Four panes](docs/screenshots/readme/four-panes.jpg) |
| Notes and highlights | Mark a word or passage, add a note, and find it again in the Notes panel. | [Multiple notes](docs/screenshots/readme/notes-panel.jpg) |
| Read aloud | Keep playback controls close while reading, and continue playback in the Library. | [Reader playback](docs/screenshots/readme/read-aloud-playing.jpg) · [Library player](docs/screenshots/readme/read-aloud-library.jpg) |
| Local voice models | Browse Kokoro, Piper, Kitten, and Phonon engines, build choices, voice counts, and download sizes. | [Model catalogue](docs/screenshots/readme/local-voice-settings.jpg) |
| AI reading tools | Explain words in context, summarize, translate, simplify, and organize a library. | [AI guide](docs/assist.md) |
| CEFR vocabulary | Shade words above a chosen level and open an in-context dictionary or AI lookup. | [Dictionary lookup](docs/screenshots/readme/cefr-word-lookup.jpg) · [AI-powered lookup](docs/screenshots/readme/cefr-ai-lookup.jpg) · [Study guide](docs/study.md) |
| AI model setup | Choose providers and models by task, including hosted and OpenAI-compatible local services. | [Model settings](docs/screenshots/readme/ai-model-setup.jpg) · [AI guide](docs/assist.md) |
| Theme presets | Customize modes, tints, textures, grain, and individual panes. | [Theme comparison](docs/screenshots/readme/themes-comparison.jpg) |

## Screenshots

### Reading surface

![Pride and Prejudice in a fit-page spread with the Thumbnails sidebar open](docs/screenshots/readme/reader-default.jpg)

### AI model setup

![AI model settings showing a provider model and its task assignments](docs/screenshots/readme/ai-model-setup.jpg)

### CEFR highlighting and lookup

![The dictionary lookup for “acknowledged,” showing its part of speech, CEFR level, and French translation](docs/screenshots/readme/cefr-word-lookup.jpg)

#### AI-powered lookup

![An assistant-generated sample lookup for the same highlighted word, “acknowledged,” with meaning, synonyms, and usage examples](docs/screenshots/readme/cefr-ai-lookup.jpg)

### Notes that stay with the book

![The Notes panel showing three saved, context-specific notes linked to highlighted passages in Pride and Prejudice](docs/screenshots/readme/notes-panel.jpg)

### Read aloud

![The read-aloud player open over a two-page spread of Pride and Prejudice](docs/screenshots/readme/read-aloud-playing.jpg)

![The full read-aloud player continuing on the Library route](docs/screenshots/readme/read-aloud-library.jpg)

Playback continues after switching to Library.

### Local voice models

![The Read aloud settings catalogue showing Kokoro, Piper, Kitten, and Phonon model options](docs/screenshots/readme/local-voice-settings.jpg)

### Four panes

![Four reading panes: Pride and Prejudice and The Raven in single-page view, with Alice's Adventures in Wonderland and Dream of the Red Chamber in two-page spreads](docs/screenshots/readme/four-panes.jpg)

### Chinese EPUB in vertical layout

![Dream of the Red Chamber displayed in vertical, right-to-left reading order](docs/screenshots/readme/chinese-vertical-epub.jpg)

### Themes

![One Pride and Prejudice page assembled from four contiguous theme quadrants](docs/screenshots/readme/themes-comparison.jpg)

## Download

Choose a macOS, Windows, or Linux build from the
[latest release](https://github.com/codewiththiha/mareader/releases/latest), or browse
[all releases](https://github.com/codewiththiha/mareader/releases).

The macOS download is not notarized and its disk image is unsigned. If macOS blocks
Mareader, use **Open Anyway** in System Settings under **Privacy & Security**. If you
have verified the download and prefer Terminal, you can remove the quarantine flag
from the installed app:

```sh
xattr -d com.apple.quarantine /Applications/Mareader.app
```

## Build from source

Requires the pinned Rust toolchain in `rust-toolchain.toml`:

```sh
cargo run -p mareader                   # desktop app (--screenshot <out.png> or --control 0 --control-file <path>)
cargo run -p mareader-cli -- commands   # list registered engine commands
cargo run -p mareader-cli -- render book.pdf --page 0 -o page0.png  # headless page PNG
cargo run -p mareader-cli -- mcp        # stdio MCP server (headless or attached live UI)
cargo test --workspace                  # run all workspace tests
```

## Developer guides

- [Development](docs/development.md)
- [Architecture](docs/architecture.md)
- [Contributor rules](AGENTS.md)
- [Roadmap](ROADMAP.md)
- [EPUB pipeline](docs/epub.md)
- [AI integration](docs/assist.md)
- [Packaging](docs/packaging.md)
- [Verification](docs/verification.md)
- [Showcase samples](docs/showcase/README.md)

## Companion repositories

Standalone crates maintained by the same author and used by Mareader:

- [ai-pool](https://github.com/codewiththiha/ai-pool) — OpenAI-compatible Rust client with key pooling, rotation, quotas, streaming, and OS-keychain storage (`MIT OR Apache-2.0`)
- [fm-bridge](https://github.com/codewiththiha/fm-bridge) — Async Rust bindings for Apple's on-device Foundation Models framework via a Swift helper (`MIT OR Apache-2.0`)

## Core libraries and licenses

Mareader and its first-party workspace crates are licensed under the MIT License ([LICENSE](LICENSE)).

Core third-party dependencies:

- [egui](https://crates.io/crates/egui) / [eframe](https://crates.io/crates/eframe) — Immediate-mode GUI and application shell (`MIT OR Apache-2.0`)
- [hayro](https://crates.io/crates/hayro) — Pure-Rust PDF parsing and rendering (`MIT OR Apache-2.0`)
- [parley](https://crates.io/crates/parley) — Rich text layout and shaping (`MIT OR Apache-2.0`)
- [vello_cpu](https://crates.io/crates/vello_cpu) — CPU 2D vector rasterization (`MIT OR Apache-2.0`)
- [rbook](https://crates.io/crates/rbook) — EPUB container reading and writing (`Apache-2.0`)
- [candle](https://crates.io/crates/candle-core) — Pure-Rust tensor and neural inference runtime (`MIT OR Apache-2.0`)

Bundled fonts, icon geometry, and vendored components carry separate notices in [ATTRIBUTION.toml](ATTRIBUTION.toml).
