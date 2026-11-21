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
| CardWriter | `crates/study/src/mode/cards.rs` | Who writes a deck's backs: `write` hands back a `CardJob` polled with `take`, cancelled by drop. Assist's `Assist::cards` and the scripted `Practice` implement it, so the engine never names assist. |
| Backs | `crates/engine/src/study_mode/backs.rs` | Backs written per word, sentence hash and writer id for the session, shared by every deck through `Writer`; a deck asks only for the misses. |
| Judge | `crates/study/src/mode/judge.rs` | Who judges a recall's answer: `judge` hands back a `JudgeJob` polled with `take`, cancelled by drop. Assist's `Assist::judge` and `Practice` implement it. `Once<T>` in `cards.rs` is the scripted job for both traits. |
| Pending | `crates/assist/src/service/cards.rs` | One study request on the runtime as a `CardJob` or `JudgeJob`; `AiCards::spawn` starts it on a short slot of study's model, and drop aborts it. |
| Recalling | `crates/engine/src/study_mode/recalling.rs` | The open recall: its word, the back asked through `Writer`, the judge's job; `step` takes both in and says when a correct answer ends it. |
| spot / click | `crates/ui-egui/tests/common/levels.rs` | Kittest: the screen point of a word on page 0, and a click on it. |
| Ahead | `crates/engine/src/study_mode/run.rs` | A deck made past the gate (making, ready or failed); `Run::take_ahead` hands it to the gate, `trim_ahead` drops those no longer coming. |
| space::view | `crates/engine/src/space.rs` | Studied words as rows for one filter: scope, tabs, groups, sorts, counts and stats. Space's page and its CSV read only this. |
