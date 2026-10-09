# download-core

Background downloads for an app that has to hand progress to a frontend: one
call to start, one snapshot stream to draw, one awaitable file at the end.

Built for the case a desktop app actually hits — a large dataset fetched once
over a network that drops, behind a firewall that blocks one CDN, into a
directory the app owns, on a runtime the app already runs.

```toml
download-core = { path = "crates/download-core" }   # or a git dependency
```

It is a leaf: no path dependency, no import of the host application, and no
opinion about what a download is *for*.

## The whole API in one screen

```rust
use download_core::{Downloads, Host, Job, Phase, Progress};

let registry = Downloads::new();          // one per process, Clone, Send + Sync

let receipt = registry.start(&host, Job::new(
        "cefr-dataset",                   // the id every later call and event names
        app_data_dir.join("cefr"),        // where it lands
        "https://cdn-a.example/cefr.parquet",
    )
    .mirror("https://cdn-b.example/cefr.parquet")   // a firewall-proof fallback
    .named("cefr.parquet")                          // or take the URL's own name
    .verified_by(|path| check_parquet(path))        // runs before adoption
    .on_progress(|progress| bridge(progress))?;     // a second sink beside the host's

match receipt.finished().await {                    // the downloaded output
    Ok(outcome) => convert(outcome.path),           // complete, verified, adopted
    Err(message) => report(message),
}
```

```rust
registry.pause("cefr-dataset");     // stop reading; the partial stays
registry.resume(&host, "cefr-dataset")?;  // continue from the bytes on disk
registry.cancel("cefr-dataset");    // stop for good; the partial still stays
registry.remove("cefr-dataset");    // drop the record, the partial and the file
download_core::discard(&dest);      // the same sweep for a record that is gone
registry.status("cefr-dataset");    // Option<Progress>
registry.list();                    // Vec<Progress>, for a booting frontend
registry.outcome("cefr-dataset");   // Option<Result<Outcome, String>>, kept after the end
```

`Host` is the only thing an app has to supply — an executor and an event sink:

```rust
impl Host for TauriHost {
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
        tauri::async_runtime::spawn(task);
    }
    fn publish(&self, progress: &Progress) {
        let _ = self.app.emit("download-progress", progress);
    }
}
```

The transport sleeps on `tokio::time`, so the executor has to drive a tokio
timer. Tauri's `async_runtime` does.

## The snapshot a frontend draws

`Progress` is `Serialize` + `Deserialize`, camelCase on the wire, with the phase
as a lower-case string:

```json
{
  "id": "cefr-dataset",
  "phase": "downloading",
  "received": 1834567,
  "total": 2939324,
  "source": 1,
  "attempt": 1,
  "speed": 842310.5,
  "etaSecs": 1,
  "message": null,
  "path": null,
  "cached": false
}
```

| phase | what it means | what a UI draws |
|---|---|---|
| `preparing` | the record exists, no byte has moved | indeterminate |
| `downloading` | bytes are arriving | `percent()`, `speed`, `etaSecs` |
| `retrying` | an attempt is spent, a backoff is running | `message` + the last bar |
| `paused` | the caller stopped the read | the bar **as it was** — a paused download that reads 0% looks lost |
| `verifying` | every byte is here, the hook is running | indeterminate |
| `done` | adopted at `path` | `cached` says whether a byte moved |
| `failed` | every mirror is out of attempts | `message` |
| `cancelled` | the caller asked to stop | offer a resume-from-partial |

`received` and `total` survive a pause and a retry, so the bar never jumps
back to zero. `Progress::percent()` is the whole percent, and never passes 100
even when a server under-declares.

## What it does when the network is having a day

**Mirrors.** Sources are tried in the order they are listed. A mirror is spent
on a permanent answer (a 4xx that is not 408 or 429) at once, or after
`ATTEMPTS_PER_SOURCE` transient ones. The attempt that lands reports its index
as `source`, so a UI can say which CDN answered. `mirror` adds one and
`mirrors` adds a list; both take any `AsRef<str>`, so a `&[&str]` of hosts and
a `Vec<String>` built at runtime are the same call. An empty list is a
one-source job, which is what a feature with a single authoritative host
writes — and adding a second host later is one line, not a refactor.

**Retries.** Transient means: a connect or stream error, a 5xx, a 408, a 429, a
416 with a partial held, or a body that ends before its declared length. The
backoff doubles from 500 ms to an 8 s ceiling, and is slept in 150 ms slices so
a pause or a cancel lands *during* it rather than after it.

**Resume.** The partial is `<dest>.part` and a sidecar `<dest>.dmeta` holds the
validator the serving mirror gave (`ETag`, else `Last-Modified`). A resume asks
`Range` **and** `If-Range`, so a resource that changed answers `200` and the
transfer restarts instead of appending a new tail to an old head. A partial
with no sidecar — or a weak `W/` etag, which `If-Range` does not accept — is
discarded rather than trusted. This is what makes a resume across an app restart
safe, and it is covered end to end in `tests/http.rs`.

**Adoption.** Bytes only ever land at `<dest>.part`. The verify hook runs
off-runtime, and only a body that passes is renamed onto the destination. A
reader therefore never sees a half-written file at the final name, and a body
that fails its check leaves neither the destination nor the partial behind.

**Cache.** If the destination already exists and passes the hook, the download
is `done` with `cached: true` and no request is made at all. `force(true)`
overrides it. A destination that *fails* the hook is deleted and re-fetched,
because a file that is not what the hook expects is not this download's.

**Stalls.** `connect_timeout` is 15 s and `read_timeout` is 60 s: a mirror that
accepts and then says nothing is spent, not waited on.

**One transport per id.** `start` refuses an id whose task is still running, so
a double-click cannot double-fetch. A finished or paused record is replaced,
and whoever waited on the old receipt is told it was superseded instead of
hanging.

**Bounded registry.** At most `SLOT_CAP` (32) records; the oldest finished ones
are evicted first. A long-lived app that starts a download per session does not
grow a map forever. Files are never evicted, only records.

**Names.** `file_name` is checked, not trusted: a separator, a traversal
component, a NUL or a colon is refused, so a name that arrives over a wire
cannot write outside its directory. Only `http` and `https` are fetched.

## The edges that are tested

`tests/http.rs` serves real bytes over a real `127.0.0.1` socket and holds the
range rules a mirror owes, so these are not stubs:

- a whole body is verified, adopted, and leaves no partial or sidecar behind
- a body cut short resumes from exactly the bytes that landed
- a resource that changed between hits restarts instead of appending
- a refused mirror hands the file to the next one, reporting `source: 1`
- a 404 fails on the first hit without a backoff — an answer is not an outage
- a 503 is asked again after the backoff, on the same mirror
- a body that fails its hook is never adopted and never left behind
- a complete file on disk is a cache hit with zero requests, and `force` isn't
- a pause keeps its byte count on the wire and a resume finishes the file
- a cancel ends the receipt, keeps the partial, and the next start resumes it

## What it deliberately does not do

- **No checksum of its own.** `verified_by` is the hook; a size, a magic number
  and a SHA-256 are all the caller's one line, and only the caller knows which
  one the file deserves.
- **No persistence across processes.** The registry is in-memory. The partial
  and its sidecar are on disk, which is what makes a restart resumable; the
  record is re-created by the next `start`.
- **No concurrency cap.** One transport task per id, and ids are the caller's
  to mint. A caller that starts forty starts forty.
- **No proxy or TLS configuration.** TLS is `rustls` with the bundled webpki
  root store — no OpenSSL, no platform trust store. Anything else is a
  `reqwest` decision, and `reqwest` is a public dependency of this crate.
