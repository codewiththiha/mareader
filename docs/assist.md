# AI assist (`crates/assist`)

The study tools ask an AI for answers: Explain today, more tasks later.
`mareader-assist` keeps three things apart so each can change alone:

| Piece | What it is | Change it to… |
|---|---|---|
| **Task** | a `Prompt` (system text, the reader's text, a JSON Schema) and how to read the answer | ask something new |
| **Backend** | sends a `Prompt`, returns JSON, reports partial answers | add a provider |
| **Service** | `Assist`: a runtime off the UI thread, clients, keys, the settings' jobs | change how the app holds it all |

Every provider gets the same `Prompt`. Nothing in a task knows who
answers, and nothing in a backend knows what was asked.

## The seam

```rust,ignore
pub trait Task: Send + Sync {
    type Output: Send;
    fn prompt(&self) -> Prompt;
    /// A partial answer may lack fields; a whole one may not.
    fn read(&self, answer: &Value, whole: bool) -> Option<Self::Output>;
}

pub trait Backend: Send + Sync {
    fn ask<'a>(&'a self, prompt: &'a Prompt, partial: Partial<'a>)
        -> BoxFuture<'a, Result<Value, Failure>>;
}

// Any task on any backend; `partial` sees each readable answer so far.
let info = run(backend, &Explain { word, context }, &|p| show(p)).await?;
```

Errors are `mareader_study::Failure`: a `Why` the card can explain
(no key, keys turned away, offline, busy, a model to check…) plus a
short detail from the provider.

## Adding a task

Three places; the task's row gives the rest (its model, queue place,
retry and fallback):

1. **A file with the Task**: a struct with what it needs, its
   `prompt()` and its `read()`.
   - `prompt()`: system text, then the reader's text through `quote()`,
     which flattens it into one JSON string so it reads as data, not
     instructions. The schema must be closed (`additionalProperties:
     false`), list every field in `required`, and use no `$ref`. Apple's
     guided generation and OpenAI's strict mode both need that.
   - `read()`: turn the JSON into the output, leniently. Models wrap
     objects and send lists as comma-separated strings; `Explain::read`
     shows how. Return `None` for a whole answer missing what matters.
2. **A `TaskId` and its row** in `TaskId::spec()`
   (`crates/settings/src/ai_task.rs`): label, what it is busy doing,
   class, retry, and the task it follows. The compiler then asks for the
   arms that list every task: where Settings › AI shows it (`place()` in
   `settings_window/ai.rs`) and how `tests/conformance.rs` starts one.
3. **A call to `ask()`** where the answer is wanted: the dispatcher
   routes it, queues it by class and retries it by its row. The `step`
   asks one model through `retry::attempt(backend, &task, partial,
   retry)`; `Assist::rewrite` is the shortest example.

Every backend already works with it. See
[Tasks as data](#tasks-as-data-schema-v8) and
[The queue](#the-queue-who-runs-who-waits-phase-4).

## Adding a provider

Implement `Backend::ask` for it, stream if it can, and map its errors to a
`Failure`. Two exist:

- **`Apple`**, through the [fm-bridge](https://github.com/codewiththiha/fm-bridge)
  helper: the schema goes in as guided generation, snapshots stream as
  partials. Only on macOS 26 with Apple Intelligence; `probe()` says why
  not. Packaging the helper is in `docs/packaging.md`.
- **`Compat`**, any OpenAI-compatible server through
  [ai-pool](https://github.com/codewiththiha/ai-pool), which brings keys,
  rotation, retries and streaming. Its `structured` request sends the
  schema as `json_schema` and in the system text. If the server refuses
  the format, or the answer has no JSON or misses the schema (Ollama
  Cloud accepts a schema and ignores it), it asks again with `json_object`,
  then with one plain prompt that spells out the reply. The mode that
  worked is kept per address and model in `ai/json-modes.json`. Answers
  are read leniently: keys ignore case, and names like `definition` or
  `examples` count. An unreadable answer names the keys it had.

A server-shaped provider usually needs only a `Provider` preset in
`crates/settings/src/ai.rs` (address, default model, whether it takes
keys). Anything else implements `Backend` and gets a match arm in
`Assist::backend`.

## Providers and settings

A provider is a server that saved models run on. Until schema v4 the
settings held one chosen provider and its model; v5 replaced that with
saved models that tasks pick from (see "Saved models and tasks" below).

`AiSettings` holds the saved models (`presets`), the tasks (`tasks`) and
an `Endpoint` per server provider. An `Endpoint` holds only the address;
blank means the provider's default. Keys are kept per provider (see
Keys), so every saved model on one provider shares them.

| Provider | Answers through | Keys | Blank model means |
|---|---|---|---|
| Apple Intelligence | the fm-bridge helper (macOS only) | — | the system model |
| OpenAI, Gemini, OpenRouter | their OpenAI-compatible APIs | required | a set default, e.g. `gpt-4.1-mini` |
| Ollama, LM Studio | the local server | — | the server's first model |
| Custom | any OpenAI-compatible address | optional | the server's first model |

A saved model's blank name shows as the provider and model ("Ollama ·
qwen3"). There is no Automatic provider any more: a new Mac starts with
an Apple Intelligence model explaining words, other systems with none.

Settings are only saved, never checked, as they are typed. Addresses
(http or https only), keys and models are checked when used, and the
answer says what's wrong.

## Keys

- One encrypted SQLite store, `ai/keys.sqlite` in the app's data folder.
  Each provider's keys are a separate scope.
- The master key sits in the system keychain under the app's bundle id, or
  in `ai/master.key` where there is no keychain. A marker file records the
  keychain choice, so a locked keychain gets reported, never quietly
  replaced by a new key that would orphan the store.
- With several keys, requests take turns. A key the provider rejects is
  set aside with the reason; a rate-limited key rests and comes back. The
  AI tab adds, sets aside, brings back and removes keys, and shows only a
  key's ends.
- A pasted key must be one ASCII word, up to 1024 characters.

## The service in the app

`Assist::new(root, waker)` starts a small Tokio runtime. The UI owns it
through `crates/ui-egui/src/ai.rs`:

- each frame, `follow(&settings.ai)` takes in finished jobs and, when the
  AI settings changed, follows them and hands the engine its
  `Answerers` in one call (`Session::study_set_ai`);
- the AI tab sends `Job`s (probe Apple, list keys or models, add, set
  aside, bring back or remove a key, try the provider) and draws the
  `Board`, plain data that each finished job updates. Then `waker`
  repaints.

`Explainer::explain` runs on the study tools' worker thread and blocks on
the runtime there, never on the UI thread. The browser build has no
assist: the AI tab is hidden and Explain says it can't answer.

### Live settings (Phase 5)

Every port reads the settings when its job starts, not when it was made.
`Assist::configure` swaps one `Arc` in the `Dispatcher` (`follow`), with
each task's route under the new settings. The explainer, the card writer
and the judge hold the dispatcher, not a model, so a port made before a
change asks the new model on its next job. A fourth port added the same
way can't keep asking the old model.

- **Who answered.** `Explainer::explain_by` returns the answer and the
  model that gave it, from `Event::Started`; the word card's small print
  shows that one, even after a fall-through to a fallback.
- **Cached backs.** A deck, and a recall's back, file what comes under
  the writer's `id()` read when they were asked. Study cards ask only
  their first model, so that is the model that wrote them.
- **One pending answer.** The engine sees every AI job as
  `mareader_study::pending::Pending`: `news()` gives `Partial`, then
  `Done`. `Writing`, `CardJob` and `JudgeJob` are aliases of it, and
  assist's `Running` implements it, so the engine still never names
  assist.

### See it working (Phase 6)

- **Jobs list.** `Dispatcher::jobs()` (`Assist::jobs`) lists every job
  queued or running as a `JobInfo`: task, saved model, running or
  waiting, and since when. Those running come first, then the line in
  its order. Settings › AI models draws a line under each model's row,
  "AI Dictionary · running 4 s" or "Study cards · waiting 12 s", and the
  seconds tick while any line shows.
- **Cards as they are written.** The card writer passes each partial
  answer on as `Event::Partial`. `StudyCards::read` leaves a partial
  answer's last card out, as it may be half written. `DeckJob` fills
  its backs from each partial, so the waiting card and the toast count
  "Writing cards 3 of 12" as cards arrive. Only the whole answer goes in
  the cache, and it stands over what came on the way.

## Tests

- `tests/compat.rs`: streaming with the schema sent, the format
  fallback, a blank model taking the server's first, error mapping.
- `tests/service.rs`: a keyless local server; keys added, used, rotated,
  set aside, brought back and removed; providers that can't answer saying
  why; Apple Intelligence through a scripted helper (Unix).
- `tests/study_mode.rs`: a request's backs by number, and card by card
  as they stream; the model's Cards per request; the judge before decks;
  cancels.
- `tests/conformance.rs`: every `TaskId` has a spec whose `follows` ends,
  routes through its fallback and loses a removed model, waits out a busy
  model as its spec says, frees its model when cancelled, and asks the new
  model after a settings change, through ports made before it.

A small HTTP server in `tests/support` stands in for providers; it
records what it was asked. Nothing in CI calls a real provider.

## Saved models and tasks (2026-10-19)

Settings schema v5 replaces the one chosen provider described above with
saved models that tasks pick from:

- **`ModelPreset`**: an id, a name (blank suggests one from the provider
  and model), a provider, a model, and **At once**, 1 to 8 tasks at the
  same time (default 1). The same provider and model may be saved twice,
  e.g. under another name with a different At once, to give one task its
  own room.
- **`AiTasks`**: one entry per task. `explain` has `preset` and
  `language`; `organize` has `preset` (none follows Explain's model) and
  `batch`, the books per request (10 to 200, default 50).
- **`Endpoint`** now holds only a provider's address; keys stay per
  provider, so every saved model of a provider shares its keys.

Loading a v4 file moves its provider and that provider's model into one
saved model and gives it to Explain. Automatic becomes Apple Intelligence
on a Mac and no model elsewhere.

Schema v8 replaced the per-task `preset` fields and their resolvers with
one map and one `route()`: see
[Tasks as data](#tasks-as-data-schema-v8).

## Two tabs: AI and AI models (2026-10-22)

The owner split the one AI tab in two and renamed Explain:

- **AI** (Study group) is what the AI does. **Words** holds the **AI
  Dictionary**, the word card that gives a word's meaning, usage and
  examples (Explain before; `tasks.explain` and `BarItem::Explain` keep
  their saved names). Clicking its name folds open how its words are
  marked: Mark, Colour and Card spacing, moved here from Highlights.
  **Library** holds Organize library and Books per request; **Pages** and
  **More than one page** hold the page tasks. With no model saved, the tab
  opens with one button to AI models, and every task reads None.
- **AI models** (App group) is where models are made and kept
  (`settings_window/models.rs`):
  - Each row shows its provider's logo, its name once, and small print
    that says what needs a hand: no API key, every key set aside, the
    server not reachable, or a model gone from it. Pills name every task
    the model runs, fallbacks included, two at most and then a count.
  - An open model edits Name, Provider, Model, At once and **Cards per
    request** (study-card words in one request, 1 to 30, default 10; a
    deck with more is split, see `docs/study-mode.md` section AI cards), and can
    be tried, made the AI Dictionary's, duplicated under a fresh name such
    as "(2)" with the same settings, or deleted after a second click.
  - **Add a model** shows the providers as tiles; with none saved they
    show at once. Adding opens the new model and its provider's row.
  - **Providers** lists the ones in use, each a row with its standing
    ("2 keys ready", "3 models on this computer"). It opens on the address
    and keys, and opens by itself when it needs a hand.
- Messages about keys and models point to Settings › AI models; the page
  menu's "Set up AI…" opens it when no model is saved.

## The queue: who runs, who waits (Phase 4)

Every AI job goes through the dispatcher (`dispatch.rs`): `ask()` for one
request, `flow()` for the organizer's many. Both return a `Running`. In
front of each saved model is a queue (`slots.rs`), and a running job holds
a `Ticket`. Dropping the ticket, or the job, lets the next one in.

- **One capacity number.** At once × usable keys: keys not set aside, at
  least one. Keyless providers count one. The queue counts a provider's
  keys the first time it is asked, and again whenever a key list comes back
  from Settings. The engine's page runs and Settings read the same number
  (`Assist::capacity`). ai-pool rotates keys request by request, so tasks
  running together already go out on different keys.
- **Keys from one account share its limits.** OpenAI counts rate limits per
  organisation and project, Gemini per project. More keys from one account
  add room here but rarely more throughput; the AI tab says so.
- **Order.** Waited before Asked before Ahead, then first come first
  served. A running job is never stopped. Each task's row gives its usual
  class (Explain and a recall's verdict Waited, page rewrites Asked, decks
  ahead and the organizer Ahead). The reader can pick another in Settings ›
  AI › Who goes first ("Right away", "Normal", "Background"), kept in
  `AiTasks::priority`; `AiSettings::class(task)` is the one reader, and a
  job queues as an `Asker`, its task and that class. The class also picks
  the retry (`Class::retry`).
- **Nothing is turned away.** Past capacity, a job waits its turn however
  long it takes, until it gets a place or its handle is dropped. Nothing
  answers `InUse` because a model is full.
- **The reader is told.** A job that waits behind a different task makes a
  toast: "“Fast” is organizing your library and runs 1 task at a time.
  Explaining a word starts when it's free." A deck behind a deck, or a page
  behind a page, is no news.
- **Bulk work steps aside.** The organizer gives its place back between
  requests while a Waited job is in line, then queues again. With room for
  two or more, Ahead work never takes the last free place.
- **Every job falls through its route.** A job moves to its next model at
  once on Busy, Offline, TimedOut or rejected keys, whatever its class. A
  Waited job also moves on after 15 s in line; other classes wait their
  turn. On its last model a job uses its class's waits. A failure names
  every model asked. The organizer's lane moves on the same way between
  requests. The table is in `dispatch.rs`.
- **News.** `Event` carries `Queued { behind }`, `Started { by }`,
  `Partial`, `Again { why, attempt }` and `Done`. `Again` comes from a
  single request asked again; a flow's steps don't send it.
- Each saved model's summary shows its capacity, how many tasks run now,
  and which tasks use it. Under it, a line per job names what runs and
  waits (see [See it working](#see-it-working-phase-6)).

Owner's call for p4-4 (2026-11-23): the limit is per saved model, the
order is by who waits, and bulk work steps aside, so Explain works while
the library is organized whenever the model has room or the organizer
is between requests.

## How each task runs today (2026-11-23)

The checklist a new task is compared against until tasks become rows of
one table, copied from the AI architecture review. Line numbers are those
of `0ebd450`. ⚠ marks a risk, ⧉ a copy of something found elsewhere.

| Task | Impl | Start | Handle | Queue | Retry | Stream | Model |
|---|---|---|---|---|---|---|---|
| AI Dictionary (Explain) | `task.rs:85` | `rt.block_on` on the study worker thread (`service.rs:511`) | None. A blocking Explainer call | Short: waits up to 15 s | ⚠ None | Yes | `explain_preset()`, frozen in `configure()` (`service.rs:130`) |
| Organize library (Shelving, Filing) | `organize.rs:551, :664` | `rt.spawn` (`service.rs:219`) | ⧉ `Organizing` (`service.rs:426`) | Long: never waits, and nothing waits behind it | ⧉ `attempt()`: one re-ask, then waits 3 s and 12 s (`organize.rs:301`) | Progress events | `organize_preset()`, read at the call (`settings/ai.rs:508`) |
| Page rewrite (Summarize, Translate, Simplify) | `page.rs:573` | `rt.spawn` (`service.rs:272`) | ⧉ `Rewriting` (`service.rs:461`) | Short: waits up to 15 s | ⧉ A copy of that loop, plus an engine requeue ×3 on InUse (`page.rs:620`) | Yes | `page_preset(task)`, read at the call (`settings/ai.rs:514`) |
| Study cards (new) | `study_mode.rs:158` | `rt.spawn` (`service/cards.rs:109`) | ⧉ `Pending<T>` (`service/cards.rs:163`) | ⚠ Short: waits up to 15 s | ⚠ None (`service/cards.rs:130`) | ⧉ Dropped | `study_preset()`, frozen in `study_ai()` (`service/cards.rs:35`) |
| Study judge (new) | `study_judge.rs:80` | `rt.spawn` (`service/cards.rs:109`) | ⧉ `Pending<T>` (`service/cards.rs:163`) | Short: waits up to 15 s | ⚠ None (`service/cards.rs:146`) | Dropped | `study_preset()`, frozen in `study_ai()` (`service/cards.rs:35`) |
| Settings jobs (Probe, Keys, Models, Try) | `settings/ai_job.rs` | `rt.spawn` (`service.rs:328`) | None. The reply lands on the Board; no cancel | Not traced | Not traced | No | The preset is passed in |

Traced since the review: of the settings jobs only Try takes a slot (a
short one), and none retries.

Underneath every row, ai-pool retries a 5xx or 408 three times on the same
key; `compat.rs` turns the last `ServerError` into `Why::Busy`, which the
organizer and page loops wait out twice more. A failing server can be
asked (1 + 3) × (1 + 2) = 12 times for one organizer step. Phase 2 of the
AI plan settled this and gave every task a retry: see
[Retry budget](#retry-budget).

The queue columns above are history: Phase 4 replaced short and long
slots with one queue, and the engine's requeue on `InUse` is gone. See
[The queue](#the-queue-who-runs-who-waits-phase-4).

## Retry budget

One loop, `retry::attempt`, asks again for every task, following the
task's `Retry` (a plain value in the settings crate):

| Policy | Tasks | Unreadable answer | Busy, Offline, TimedOut |
|---|---|---|---|
| `WAITED` | Explain, the study judge | asked again once | one 2 s wait, then asked again |
| `AHEAD` | Study cards, the organizer, page rewrites | asked again once | waits 3 s, then 12 s |

ai-pool asks a key again by itself after a 5xx or 408: three times, from
200 ms, doubling. The `Busy` it then gives up with is marked `retried`
(`compat.rs`), and the loop doesn't wait on it again. Resting keys
(`AllKeysExhausted` with a time) aren't marked: ai-pool hasn't asked
again, and waiting is what helps.

ai-pool also steps an answer with no JSON in it down from a JSON schema
to JSON mode to a plain prompt, and remembers the first mode that gave
JSON for that model. If no mode gave any, the `BadAnswer` is marked
`retried` and not asked again: asking would replay the whole chain. If a
mode gave JSON with the wrong fields, the one re-ask goes out in that
remembered mode, a single request.

What one task can cost against a server that keeps failing:

| Failure | `WAITED` | `AHEAD` |
|---|---|---|
| 5xx or 408 | ai-pool's 1 + 3 = **4** requests | **4** |
| Busy (429, resting keys), Offline, TimedOut | 1 + 1 wait = **2** | 1 + 2 waits = **3** |
| Unreadable answer, no step-down | 1 + 1 re-ask = **2** | **2** |
| No JSON in any mode | ai-pool's 3 modes = **3** | **3** |
| Wrong fields after a step-down | 3 modes + 1 re-ask = **4** | **4** |

Pinned by `tests/retry_budget.rs`, `tests/retry_baseline.rs` and the
unit tests in `retry.rs`.

## Tasks as data (schema v8)

Each AI task is a `TaskId` with one row, its `TaskSpec`
(`settings/src/ai_task.rs`). `spec()` is an exhaustive match, so a task
without a row doesn't compile.

Class and retry below are the usual ones; the reader's pick wins.

| Task | Label | Class | Retry | Follows |
|---|---|---|---|---|
| `explain` | AI Dictionary | Waited | `WAITED` | — |
| `organize` | Organize library | Ahead | `AHEAD` | Explain |
| `summarize`, `translate`, `simplify` | Summarize, Translate, Simplify | Asked | `AHEAD` | Explain |
| `study_cards` | Study cards | Ahead | `AHEAD` | Explain |
| `study_judge` | Study judge | Waited | `WAITED` | Explain |

`doing` is the phrase a task is busy with, as "organizing your library".
`class` orders the queue (Phase 4 of the AI plan) and picks the retry.

- **`AiTasks::models`** maps each task to its saved models, first choice
  first. Per-task options (language, batch, style, start) stay in their
  own structs.
- **`AiSettings::route(task)`** is the one resolver: the task's own
  models, then those of the task it follows, then the backups, each once.
  A task with no model of its own or followed gets no backups, so None on
  the AI Dictionary still leaves the following tasks off. `model(task)` is
  its first. Every call site asks for a `TaskId`; a page task converts
  with `TaskId::from(PageTask)`.
- **Backups** (`AiTasks::backups`, `ai_backups.rs`) are one ordered list
  for every task, set in Settings › AI › Backup models: drag or the arrows
  to reorder, × to remove, "Add a backup" for more. Until the reader
  changes it the list is `null` in the file and means the first
  `Backups::AUTO` (3) saved models, oldest first, kept up to date. Once
  arranged it is stored and new models aren't added. The section shows
  from two saved models. A task's own model is skipped in it.
- **Per-task fallbacks** stay file-only: Settings shows one model per row;
  choosing one keeps any fallbacks behind it, and None clears them. They
  come before the backups.
- **Removing a model** drops its id from every list, the backups too, and
  a task's list left empty goes. `sanitize()` does the same for ids no model has.
- **A task this build doesn't know**, as a newer build's, is skipped when
  the file is read; the rest of the AI settings stay.

Loading a v7 file moves each `tasks.<task>.preset` into `models`. Study's
one model goes to both `study_cards` and `study_judge`. A null leaves the
task following Explain, and a lost id is dropped. Settings › Study shows
the two study tasks as two rows.

Shared words have one home each:

- **Prompts** (`prompt.rs`): `JSON_ONLY` ends every system prompt.
  `AS_DATA` follows each prompt's word on what it quotes. `bilingual()`
  is the rule for a meaning in another language, for Explain and the
  cards.
- **No model** (`settings::no_model(task)`): "No AI model is set for
  organizing your library. Choose one in Settings › AI." It is built from
  the task's `doing`. The service puts it in a `NotHere` failure's small
  print, and the engine and the organizer's toasts say it as it is.
- **A failure** (`Failure::said()`): its `Why` message, then the small
  print in brackets. A `NotHere` failure's small print is the whole of it.

## Workflows: the organizer

Organizing is a fixed workflow, not an open-ended agent loop: the steps
are known, so code runs them and checks every answer before the next step.
That keeps the cost predictable and the result reviewable. The plan builds
the shelf tree over batches first, then files each book; the same split
appears in the TnT-LLM taxonomy work (KDD 2024).

1. **Shelving.** Each batch of `batch` books (title and author, quoted as
   data) is shown with every shelf so far, both existing and new. The model
   answers `{"shelves": ["Fiction > Classics", …]}`. A batch may add no
   shelves.
2. **Filing.** For each batch the model sees numbered shelves and books
   `b1…`, and answers each book's shelf number; 0 leaves it on no shelf.
   Books it skipped or misnumbered are asked once more, then left on no
   shelf.

Guards, all in `organize.rs`:

- Paths split on `>` and `›` only, so names may hold `/`.
- Names match existing shelves ignoring case.
- Depth is capped at 5 (deeper ones fold into their parent), new shelves at
  120, and names at 60 characters.
- An unreadable answer is asked again once. A shelving batch still
  unreadable is skipped; a transport error fails the run.
- New shelves left empty are dropped. In the `Plan`, a new shelf's parent
  always comes before it.

**Chat-only models.** Every step is an ordinary task, so it goes through
the same `structured` request: a JSON Schema first, then `json_object`,
then a plain prompt that spells out the reply. The system text ends
"Reply with one JSON object only", and ai-pool cuts the JSON out of code
fences, `<think>` blocks or surrounding prose. Anything that doesn't read
into the expected shape counts as unreadable, so the workflow never
guesses.

**The service.** `Assist::organize(Request)` returns an `Organizing` at
once and runs on the runtime:

- It takes a place on its first model through `flow()`, waiting its turn
  on a full model, and steps aside between requests for a Waited job. A
  model that fails hands the run to the next in its route. No model chosen
  is `NotHere`.
- `poll()` returns `Organized::Progress` events (one at the start, then one
  per request, out of 2 × batches), then a final `Done` with the plan or
  the failure.
- `cancel()`, or dropping the handle, stops the run and frees its place.

The crate knows nothing of the library: books and shelves go in and come
back as `u128` keys (`BookId.0`, `ShelfId.0`). What the library does with
the plan is in `docs/library.md`, section AI organizer.

A new workflow is the same shape: write its tasks, chain them in an async
function over `&dyn Backend`, and start it from `Assist` with `flow()`.

### Pausing and resuming (2026-10-19)

A run can stop partway: a failure, the Pause button, or quitting the
app. It never starts over on its own. After every step, `organize_from`
hands a `Checkpoint` to its `save` hook. The checkpoint holds:

- the batches as split when the run began,
- whether it covered the whole library,
- how many batches are shelved and filed,
- the shelf paths named so far,
- each book filed so far, by place.

A place is `Place::Shelf(key)` for a shelf there before, or
`Place::Path` for a new one. A rename or removal is therefore noticed
later.

The service writes the checkpoint to `organize.json` under the AI
folder, whole or not at all (a `.part` file renamed over it). It keeps
it until the plan is applied or discarded.

- `Assist::resume(Request)` loads it and calls `Checkpoint::carry_on`
  with today's request. Books no longer loose drop out, and edited
  titles are asked as they are now. For a whole-library run, books
  loose since then join as extra batches. Done steps are not asked
  again.
- At the end, a book whose shelf went meanwhile is filed again. If that
  can't be done now, it is left on no shelf.
- A finished checkpoint only makes its plan. It takes no slot and needs
  no model, so a plan that waited over a restart opens even with the
  model busy or gone.
- `Busy`, `Offline` and `TimedOut` are waited out twice (`WAITS`, 3 s
  then 12 s) before the run fails, unless ai-pool already asked again
  (see [Retry budget](#retry-budget)). Other failures fail at once.
  Either way, every saved step stays.
- A fresh `Assist::organize` replaces the checkpoint only once it holds
  its place. No model (`NotHere`), or a run dropped while it waits, leaves
  a paused run as it was.
- `paused()` reads the checkpoint, `set_aside()` and `bring_back()`
  rename it to `organize.old.json` and back (for Undo), and `forget()`
  removes both.

### Its tests

- `src/organize.rs`: a scripted backend answers each step: 100 books
  in two batches, the repair round, unreadable answers and a dead server,
  paths tidied and capped in depth, and titles quoted as data.
- `tests/service.rs`: a chatty model with no JSON mode organizes books
  while holding its model; on a busy model it waits, and cancelling frees
  it.
- Resuming (`src/organize.rs`): a stopped run carries on without asking
  again; a resumed run fits the library as it is now; a finished run
  makes its plan without asking; a busy server is waited out, then the
  run pauses; a checkpoint reads back as written.
- `tests/service.rs`: a failed run is kept and resumed where it stopped.
