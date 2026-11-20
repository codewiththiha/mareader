# Shared code

Search this index and the code before introducing a new helper. The paths below
name the owning modules for shared primitives across the workspace.

| Name | Path | Contract |
| --- | --- | --- |
| ByteLruCache | `crates/engine/src/lru.rs` | Byte- and entry-bounded icon cache; eviction and capacity live here. |
| PackKey | `crates/ui-egui/src/icons/mod.rs` | One pack/size cache key for animated atlases and resting glyphs. |
| nn-core | `crates/nn-core` | Dependency-free L0 SIMD, protobuf wire and SplitMix64 primitives shared by Kokoro, Phonon and VITS. |
| parity-kit | `tools/parity-kit/src/lib.rs` | Float input, correlation, envelope, edit distance and rounded PCM WAV output for Kitten, Piper, Phonon and Kokoro parity tools. |
| tests/common | `crates/ui-egui/tests/common/mod.rs`; `crates/epub/tests/common/mod.rs` | Shared integration drivers; suite-specific fixtures and reader aliases retain their original settings. |
| test_support | `crates/library/src/test_support.rs` | cfg(test)-only opened-book, page, unsent-book and measurement fixtures for library tests. |
| counted | `crates/engine/src/library/mod.rs` | Sole counted-book label helper shared across library views. |
| preset labels | `crates/ui-egui/src/dial_deck/presets.rs` | Shared `mode_icon`, `noise_label` and `strength_percent` formatting for appearance and dial widgets. |
| entrance | `crates/ui-egui/src/motion.rs` | Shared entrance easing; callers pass their own duration. |
| off_thread | `crates/engine/src/word_marks.rs` | One result from a named thread, or in the frame when inline or on wasm; wakes once sent. Word lists, study decks and study page sizes. |
| stable_hash | `crates/engine/src/lib.rs` | Deterministic hash helper shared by Page AI, transcripts and triggers; unrelated direct `DefaultHasher` uses remain local. |
