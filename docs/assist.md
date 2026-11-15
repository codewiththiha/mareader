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

1. Write a struct with what the task needs, e.g. `Summarise { text }`.
2. `prompt()`: system text, then the reader's text through `quote()`, which
   flattens it into one JSON string so it reads as data, not instructions.
   The schema must be closed (`additionalProperties: false`), list every
   field in `required`, and use no `$ref`. Apple's guided generation and
   OpenAI's strict mode both need that.
3. `read()`: turn the JSON into the output, leniently. Models wrap objects
   and send lists as comma-separated strings; `Explain::read` shows how.
   Return `None` for a whole answer that is missing what matters.
4. Call `run(backend, &task, partial)`. Every backend already works.

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
  AI settings changed, hands the study tools a new `Explainer`;
- the AI tab sends `Job`s (probe Apple, list keys or models, add, set
  aside, bring back or remove a key, try the provider) and draws the
  `Board`, plain data that each finished job updates. Then `waker`
  repaints.

`Explainer::explain` runs on the study tools' worker thread and blocks on
the runtime there, never on the UI thread. The browser build has no
assist: the AI tab is hidden and Explain says it can't answer.

## Tests

- `tests/compat.rs`: streaming with the schema sent, the format
  fallback, a blank model taking the server's first, error mapping.
- `tests/service.rs`: a keyless local server; keys added, used, rotated,
  set aside, brought back and removed; providers that can't answer saying
  why; Apple Intelligence through a scripted helper (Unix).

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

A new task adds a field to `AiTasks`, a resolver on `AiSettings` (as
`organize_preset()`, which may fall back to another task's model), and a
row under Tasks in Settings › AI.

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
  - An open model edits Name, Provider, Model and At once, and can be
    tried, made the AI Dictionary's, duplicated under a fresh name such
    as "(2)" with the same At once, or deleted after a second click.
  - **Add a model** shows the providers as tiles; with none saved they
    show at once. Adding opens the new model and its provider's row.
  - **Providers** lists the ones in use, each a row with its standing
    ("2 keys ready", "3 models on this computer"). It opens on the address
    and keys, and opens by itself when it needs a hand.
- Messages about keys and models point to Settings › AI models; the page
  menu's "Set up AI…" opens it when no model is saved.

## Room: how many tasks a model runs

Every task holds a slot of its model's `Slots` while it runs (`slots.rs`).
A `Ticket` frees its slot when dropped, so a task that fails, finishes or
is cancelled frees it at once.

- **Capacity** is At once × usable keys: keys not set aside, at least one.
  Keyless providers count one. ai-pool rotates keys request by request, so
  tasks running together already go out on different keys.
- **Keys from one account share its limits.** OpenAI counts rate limits per
  organisation and project, Gemini per project. More keys from one account
  add room here but rarely more throughput; the AI tab says so.
- **Short tasks** (Explain) wait up to 15 s behind other short ones.
  **Long tasks** (the organizer) never wait, and nothing waits behind a long
  task.
- **A refusal** is `Why::InUse`, naming the model, who holds it, and how to
  get room: "“Fast” is organizing your library and runs 1 task at a time.
  Raise “At once” or pick another model in Settings › AI."
- Each saved model's summary shows its capacity, how many tasks run now,
  and which tasks use it.

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

- It holds a long slot named "organizing your library". A full model
  answers `InUse` straight away; no model chosen is `NotHere`.
- `poll()` returns `Organized::Progress` events (one at the start, then one
  per request, out of 2 × batches), then a final `Done` with the plan or
  the failure.
- `cancel()`, or dropping the handle, stops the run and frees its slot.

The crate knows nothing of the library: books and shelves go in and come
back as `u128` keys (`BookId.0`, `ShelfId.0`). What the library does with
the plan is in `docs/library.md`, § AI organizer.

A new workflow is the same shape: write its tasks, chain them in an async
function over `&dyn Backend`, and start it from `Assist` holding a slot.

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
  then 12 s) before the run fails. Other failures fail at once. Either
  way, every saved step stays.
- A fresh `Assist::organize` replaces the checkpoint only once it holds
  its slot. A refusal (`InUse`, `NotHere`) leaves a paused run as it
  was.
- `paused()` reads the checkpoint, `set_aside()` and `bring_back()`
  rename it to `organize.old.json` and back (for Undo), and `forget()`
  removes both.

### Its tests

- `src/organize.rs`: a scripted backend answers each step: 100 books
  in two batches, the repair round, unreadable answers and a dead server,
  paths tidied and capped in depth, and titles quoted as data.
- `tests/service.rs`: a chatty model with no JSON mode organizes books
  while holding its model; a busy model refuses, and cancelling frees it.
- Resuming (`src/organize.rs`): a stopped run carries on without asking
  again; a resumed run fits the library as it is now; a finished run
  makes its plan without asking; a busy server is waited out, then the
  run pauses; a checkpoint reads back as written.
- `tests/service.rs`: a failed run is kept and resumed where it stopped.
