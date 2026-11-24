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
| help_dot | `crates/ui-egui/src/help_dot.rs` | The "?" beside a heading: hover shows its card, a click pins it; paragraphs are (bold lead, words). Settings uses it through `section_help`. |
| counted | `crates/engine/src/library/mod.rs` | Sole counted-book label helper shared across library views. |
| preset labels | `crates/ui-egui/src/dial_deck/presets.rs` | Shared `mode_icon`, `noise_label` and `strength_percent` formatting for appearance and dial widgets. |
| entrance | `crates/ui-egui/src/motion.rs` | Shared entrance easing; callers pass their own duration. |
| off_thread | `crates/engine/src/word_marks.rs` | One result from a named thread, or in the frame when inline or on wasm; wakes once sent. Word lists, study decks and study page sizes. |
| stable_hash | `crates/engine/src/lib.rs` | Deterministic hash helper shared by Page AI, transcripts and triggers; unrelated direct `DefaultHasher` uses remain local. |
| Pending | `crates/study/src/pending.rs` | Every AI job as the engine sees it: `news()` gives `Partial`, then `Done`; `take()` skips to the end; `queued()` says it waits in line for a model; drop cancels. `Writing`, `CardJob` and `JudgeJob` are aliases; assist's `Running` and the scripted `Once<T>` implement it. `Answerers` is the engine's one port. |
| CardWriter | `crates/study/src/mode/cards.rs` | Who writes a deck's backs: `write` hands back a `CardJob` (a `Pending`), cancelled by drop. Assist's `Assist::cards`, its `SampleCards` (a model's long JSON answer, for shots) and the scripted `Practice` implement it, so the engine never names assist. |
| Backs | `crates/engine/src/study_mode/backs.rs` | Backs written per word, sentence hash and writer id for the session, shared by every deck through `Writer`; a deck asks only for the misses. |
| Judge | `crates/study/src/mode/judge.rs` | Who judges a recall's answer: `judge` hands back a `JudgeJob` (a `Pending`), cancelled by drop. Assist's `Assist::judge` and `Practice` implement it. `Once<T>` in `cards.rs` is the scripted job for both traits. |
| Dispatcher | `crates/assist/src/dispatch.rs` | What every AI job starts from: the runtime, the queue and the settings followed, swapped as one `Arc`. Ports hold a clone and read the route when a job starts. `jobs()` lists what is queued and running, as `JobInfo`, for Settings. |
| Asker / Backups | `crates/assist/src/slots.rs`, `crates/settings/src/ai_backups.rs` | `Asker` is a task and the class it queues as; `Followed::asker` reads the reader's pick, and `From<TaskId>` gives the usual class. `Backups` is the global ordered fallback list: `ids()` resolves automatic or arranged, `AiSettings::backups()` the presets. |
| Recalling | `crates/engine/src/study_mode/recalling.rs` | The open recall: its word, the back asked through `Writer`, the judge's job; `step` takes both in and says when a correct answer ends it. |
| spot / click | `crates/ui-egui/tests/common/levels.rs` | Kittest: the screen point of a word on page 0, and a click on it. |
| Ahead | `crates/engine/src/study_mode/run.rs` | A deck made past the gate (making, ready or failed); `Run::take_ahead` hands it to the gate, `trim_ahead` drops those no longer coming. |
| space::view | `crates/engine/src/space.rs` | Studied words as rows for one filter: scope, tabs, groups, sorts, counts and stats. Space's page and its CSV read only this. |
| space::notes | `crates/engine/src/space/notes.rs` | Every book's highlights, notes and bookmarks for one `NotesFilter`: a group per book, chip counts and the rail's shelves. Space's Notes page reads only this; `Room` says which Space page shows. |
| reading_order | `crates/engine/src/notes_panel/mod.rs` | Where a `Spot` falls in its book, so notes list as the book runs; the notes panel and Space's notes sort by it. |
| Library::every_mark | `crates/library/src/marks.rs` | Every book's living marks, unordered; `marks(book)` gives one book's, oldest first. |
| space rail / tabs | `crates/ui-egui/src/space/rail.rs`, `top.rs` | `rail::books` lists All books then each `Shelf`; `top::tab_row` draws tabs with counts over a hairline. Both Space pages use them. |
| thumbnails::small | `crates/ui-egui/src/thumbnails.rs` | One page drawn small at a rect in a given look, as the thumbnail grid draws it, pictures kept; the caller adds the quads with `PagesCallback`. A link's peek (`pdf_view/overlay/peek.rs`) uses it. |
| read_aloud::tab | `crates/ui-egui/src/read_aloud/tab.rs` | A toast's words grown out of the player's top as one body: same fill and edge, concave feet, shadow cast under the pill. `toast::island_note` hands it the words each frame. |
| settings pickers | `crates/ui-egui/src/settings_window/ai.rs` | `model_row`, `placed` and `language_picker`, `pub(super)` for the AI and Study mode tabs; a task's row names what an unset choice follows. |
| Settled | `crates/ui-egui/src/vocab.rs` | A recalled word by page and first character, drawn in its own reserved look by `vocab::place`/`add` and never hit or hovered. |
| keep_under | `crates/ui-egui/src/z_order.rs` | An area newly shown stays under its tier's layers already open; they are lifted back the pass after egui raises it. Study mode's veil uses it. |
| Known | `crates/engine/src/study_mode/known.rs` | Memorized words as a deck skips them; a key without a class matches every class. The library's and a run's own are merged per deck. |
| skill_card | `crates/ui-egui/src/skill_card.rs` | A temporary plain-text answer card pointing at its text: `width_for(chars)` sizes it by the text's length; `in_view` says whether an ask's place is on screen (for the landing toast). |
| study::offer_skills | `crates/ui-egui/src/study/bar.rs` | Hands the frame's custom skills to the selection bar's Ask AI list through context data, so the bar's signature stays as it is. |
| Ask::asked_chars / Anchor | `crates/engine/src/skills.rs` | A skill's ask: its anchor (selected spans or a page spot) and `reveal()`, the page and characters to scroll back to. |
| skill_menu | `crates/ui-egui/src/skill_menu.rs` | Skills in any menu: `builtins` (three side by side) and `custom` (folded row, hover list beside it, type-to-find, Escape folds first). `holds_escape`/`owns` let the host menu leave Escape and presses to it. |
| glow | `crates/ui-egui/src/glow.rs` | The AI glow round any outline while the AI works: `state(ctx, id, working)` fades it in, flares and fades it out; `paint(ctx, painter, tokens, rect, radius, glow)` draws it in the look's `Tokens::glow` (from `Appearance::glow_palette`) and books its own next frame; `spill` says how far it reaches. AI marks use it (`study::marks::glow`), and so do thumbnails the AI writes for. |
