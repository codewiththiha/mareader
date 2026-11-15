# G4 — Law 1 staged plan (not yet executed)

Owner chose `plan` for Law 1 at 52c8958: Law 3 and Law 2 to green first, then a
staged plan for the owner to pick stage one from. This document is that plan;
no state has moved yet. Branch tip at `072ef69` (green `437...`) holds Law 3
complete (`CommandSpec { id, label, params, enabled, parse }`, 63 rows, `execute_id`)
and Law 2's door (`Session::execute_id`) with the 16 direct mutator calls still
in the front end (see G4.md "Reported, not done" — corrected from eight).

Law 1: *`Session` owns the app's state. The law lists 13 kinds; `session.rs:27`
holds six of them (settings, pane tree + focus + recent, route + `dict_from`,
open documents as `LastDocument`, the host, the save waker/events). The other
seven live in front-end controllers behind `View`'s 52 methods, and 35 keys stay
up because a key that seeds a controller cannot be an engine command until the
controller is engine state.*

The door is `View::session(&mut self) -> &mut Session` (`engine/src/view.rs:46`);
impls are `Front<'_>` (`ui-egui/src/app/apply/view.rs:31`) and `Headless`
(`engine/tests/common/mod.rs:76`). Moving state means shrinking `View` and
growing `Session`, one kind per stage, each stage green on its own.

## The seven kinds, as they sit today

| Kind (law's name) | Where it lives today | `View` methods that carry it | Why it stays up |
| --- | --- | --- | --- |
| **1. Documents' model** | `Session` in `crates/session` holds docs, but `engine::Session` holds `pane::placement`, `page`, `SpinePlace` view state and `LastDocument` only | `open`, `close`, `focus`, `active`, `recent`, `document`, `place`, `book` — 8 of 52 | Each pane's view is engine state already; the model (`Document`, `PageKey`, tile cache) is L3 and correctly below, but the front-end's `documents.rs` (open/place/spine/book) still owns the "which doc in which pane" table that `Session` should own after G4's save move |
| **2. Library** | `ui-egui/src/app/library/*` (`State`, `Shelf`, `acting.rs`) + `engine/src/library/{seed,shelving,filing}` | `library`, `library_view`, `forget`, `open_book`, `place_book` — 5 | `LibraryAction` already exists (12 variants added at `8653314`); the controller holds the live `State` (scans, shelves, picks) and its worker. Keys `open`, `select`, `remove` became those actions but still need the controller at dispatch |
| **3. Marks and notes** | `engine/src/marks.rs` is empty; `ui-egui` holds `marks`, `notes`, `highlights`, `bookmarks`, `selections` via `pane::marks` + `engine/src/document` text layers | `highlight`, `note`, `bookmark`, `select`, `place_read_marker`, `dismiss_read_markers` — 6 | Each highlight is a `Pen + Rgb + Spot + pane`; notes are `Anchor + text`. No engine store yet |
| **4. Read-aloud** | `engine/src/read_aloud/*` holds `ReadFrom`, `Spot`, `ReadState` but the front-end's `read_aloud/mod.rs` drives `Voicer`, `Spoken`, `Feed` and the 7 `Voicer` impls | `read_aloud`, `toggle_read_aloud`, `stop_read_aloud`, `say_word`, `stop_saying`, `place_read_marker`, `try_voice*` — 7 | `View::read_aloud` is the dispatcher for all 7; settings (`ReadSettings`) are in `Session` but the running `Reading` is not |
| **5. Page-AI results** | `engine/src/page_ai/{state,ledger,extract,groups}` hold AI's ledger but `ui-egui` holds the `PageAI` controller, `Task`, `Handle`, `Rewritten` mapping | `page_ai`, `ai` — 2 | `PageAiAction` exists but the controller's `Task` holds a `Handle` (a future). AI jobs are L3 work; only their results need to be session state |
| **6. Dictionary / vocabulary** | Front-end `dict.rs`, `vocab.rs`, `study/*` hold `DictionaryPacks`, `Vocabulary`'s `Cefr` dataset, `StudyState` | `dict`, `dict_window`, `vocab`, `pronunciation`, `pursue`, `study`, `explain`, `look_up` — 8 | `vocab.attach` (G4's `647c102`) already moved the dataset folder as a command, but the controller still holds the attached `Dict` handle |
| **7. Running jobs** | `engine/src/jobs.rs` holds `Job` enum (7 variants: `Probe`, `Keys`, `AddKey`, `RemoveKey`, `BanKey`, `RecoverKey`, `Models`, `Try`) but execution is `ui-egui` `jobs/mod.rs` + `app/frame/input.rs` drain | `ai_job`, `report`, `drain`, `take_failure`, `touch`, `keep`, `settle`, `flush` — 8 | `Session::drain()` already drains engine events; jobs are the front-end's tail of that. `View::session().drain()` is called in `apply.rs:29` — the only non-command mutator that is not a lifecycle cadence |

52 methods in `View` today; after all seven move, `View` is ~9 methods
(`session`, `set_appearance`, `open_settings`, `set_clipboard`, `reveal`,
`open_external`, `state_dir`, `cache_dir`, `host`). Each stage deletes the
methods its kind owns and replaces their dispatch arms with `session.apply`
or `session.mutate` — the law's "only through `execute`" becomes literal.

## Staging, in dependency order (7 stages, ~1 commit each)

**Stage 1 — Marks & notes** (smallest, most independent, 3 files, ~200 lines).
`Session { marks: Marks }` where `Marks { pane -> Vec<Highlight>, bookmarks,
notes }`. New commands `mark.*` already have rows (`mark.bookmark`,
`mark.highlight`, `mark.select`, `mark.note` plus `read.marker`); move their
`study::run` arms into `engine/src/marks.rs` and delete `View::{highlight,note,
bookmark,select,place_read_marker,dismiss_read_markers}` (6). Fixtures that
write a mark (`pick`, `highlight`, `bookmark`) become `engine::options::parse`
cases. Gate: `View` 52→46, no `View` method names `marks` in `crates/ui-egui`.

**Stage 2 — Documents' pane table** (unblocks Law 2's `close`, `show`,
`split_at`; 4 files, ~300 lines). `Session { docs: Docs }` where `Docs` is the
`pane -> (DocId, place, spine, page)` map currently in `ui-egui/documents.rs`
plus `LastDocument`. Move `open`, `place_book`, `open_book`, `close` dispatch
into `engine/documents.rs` (which already has `OpenRequest`). `View::{open,
close, book, path}` go; `pane.swap/move` keep `Tree` but read `Docs` for their
room check. Gate: `session.close` call sites in `documents.rs:776,845` disappear
(they become `execute_id("reader.close",…)` or `ClosePane`).

**Stage 3 — Library** (largest, but `8653314`'s `LibraryAction` already staged it;
5 files, ~400 lines). `Session { library: library::State }` — move the `State`
struct (currently `ui-egui/src/app/library/state.rs`) into
`engine/src/library/state.rs` beside `seed.rs`. The `Library` controller's
worker (`scan`, `shelving`) becomes an engine `Pool` job like `session`'s docs;
`View::library` goes and `dispatch`'s `Library` arm becomes `session.apply`.
Stage 2's `Docs` gives the `open` side that today reads the library model for
`select`/`remove`/`open` — the reason those three keys became `LibraryAction`
asks rather than commands. Gate: `View` 46→~38, `crates/ui-egui/src/app/library`
shrinks to a view (`LibraryView`).

**Stage 4 — Read-aloud** (most valuable for headless `mareader-cli say`; 6 files,
~500 lines). `Session { reading: Reading }` where `Reading` is the `Spot +
ReadFrom` plus the `Voicer` handle. The `Voicer` trait stays L0–L1 but its
driver (`Feed`, `Spoken`, `Saying`) moves to `engine/src/read_aloud/state.rs`;
`View::{read_aloud,toggle,stop,say_word,stop_saying,try_voice*}` (7) go and their
commands become `session.apply` with `now`. `Dials::dial` in `tests/common` keeps
its `set_settings(whole)` contract, but read-aloud settings now move like marks
did. Gate: `try_voice` samples still play (cache_dir via `Host`).

**Stage 5 — Page-AI results** (shortest, 3 files, ~250 lines). `Session {
page_ai: PageAiLedger }` — the `Rewritten` map currently in
`ui-egui`'s `page_ai` controller. `View::{page_ai,ai}` go; `ai.page` (`stop_pages`)
becomes `session.apply`. The AI's `Source` (extracted text) already lives in
`engine/page_ai/extract.rs` and stays there; only its results move. Gate: `specs.rs`
already pins `ai.page`'s row.

**Stage 6 — Dictionary / vocabulary** (needs Law 3's `vocab.attach` — already
there — plus a session-owned dataset handle; 5 files, ~350 lines). `Session {
dictionaries: Dictionaries, vocabulary: Vocabulary }` where each is the
already-existing engine handle (`dict::State`, `study::Vocabulary`) plus their
`AiJob` probe. `View::{dict,dict_window,vocab,pronunciation,pursue,study,
explain,look_up}` (8) go; `dict.act` / `study.act` / `vocab.download` become
`session.apply`. `cefr`'s `VocabAttach` is the pattern for `dictionaries`'s
folder. Gate: `ui-egui/tests/option_keys.rs` no longer lists `dictionaries`
as front-end-only (it will be engine).

**Stage 7 — Running jobs** (last, because every other stage queues a job;
4 files, ~300 lines). `Session { jobs: Jobs }` where `Jobs` is the `Job` enum's
executor currently in `ui-egui/src/app/frame.rs` (`keep`, `settle`, `flush`,
`touch`, `take_failure`, `report`, `drain`). Move the cadence (`keep` →
`save(now) -> Option<f64>`, `settle` → `note+flush`, etc.) into `engine/save.rs`
plus a new `engine/jobs.rs` scheduler; `View::drain` becomes
`session.drain()` called from `MareaderApp::logic` and `View::report` becomes
`session.report(Toast)`. The eight `apply.rs/frame.rs/setup.rs` mutator calls
that are not commands — `keep`, `settle`, `flush`, `touch`, `take_failure`,
`drain`, `report`, `import` — become `Session`'s own cadence called without a
`View`; only `apply`'s `session.apply(Command)` remains as `execute`'s tail.
Gate: `grep session\.\(keep\|settle\|flush\|touch\|take_failure\|report\|drain\)
crates/ui-egui` empty; `View` is `session(&mut self)` plus chrome/dialog hosts.

Total delta: ~2500 lines moved (as Law 1's `everything` depth foretold), 52→~9
`View` methods, one new `Command` per moved key (already there for most), and
no visible behaviour change for the 906 corpus uses — each stage keeps the key
spelling, only what answers it moves, as G4 did for `panes`, `focus`,
`pinned` etc. Each stage is its own `cargo test` green; the final stage's
`cargo xtask trapped` still 0 and `layers` still 31 crates, because no new
crate or layer is added.

## What to pick for stage one

Three viable first stages; owner picks one:

| Stage one | Lines | Risk | Why pick it |
| --- | --- | --- | --- |
| **Marks & notes** (proposed) | ~200 | low — no worker, no I/O, only in-memory highlights | Fastest green, proves the `Session { X }` pattern, frees 6 `View` methods, and the `mark.*` commands already have rows so no new `Command` variant |
| **Documents' pane table** | ~300 | medium — touches save/load and `LastDocument` | Unblocks Law 2's `close`/`show`/`split_at` directly, so Law 2 and Law 1 close together; but it moves the file that G4's save just took ownership of |
| **Read-aloud** | ~500 | higher — threads, `Host::cache_dir`, `Voicer` lifetimes | Most user-visible for `mareader-cli say` and headless tests, but needs `Host` alive and is the largest stage after Library |

Recommended: **Marks & notes** first, then Documents' table, then the rest in
dependency order above. No manifest moves, no `Cargo.lock` hand-edit, no new
`Command` variant in stage one — only fields and `View` methods.

## What stays in the front end

After all seven, `MareaderApp` is `{ session: Session, ui: UiState }` where
`UiState` is chrome: `toasts`, `sidebar`, `find`, `platform`, `chromeless`,
`menu`, plus the three allowed toolkit-free files' tables (`lib.rs`,
`icons/pack.rs`, `dial_deck/labels.rs`). `Session` is the app's state, `View`
is the host + dialogs, and `Command` is the only way to change state — which is
what Law 1 says and what Law 2's second sentence ("only through `execute`")
becomes once the 16 direct calls are routed through it (Law 2's fixup already
has `execute_id`; see `execute.rs:37` and `docs/slices/G4.md` Law 2 bullet).
