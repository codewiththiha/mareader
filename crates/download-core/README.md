# download-core

Resumable background downloads for any feature: one id, one or more direct
links to one file, progress a frontend can render, pause and resume, and the
finished path handed back at the end.

A feature hands over a `DownloadRequest`, gives the crate the `Host` its app
already has — an executor, an event channel, a data directory — and gets
every snapshot through a `ProgressHook`, the host's channel, or both. There
is nothing to install, no global to reach for, and no runtime of the crate's
own: it spawns on the host the app already runs.

## Quickstart

```no_run
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use download_core::{DownloadRequest, Downloads, Host, Progress, ProgressHook};

/// Your app: an executor, an event channel, a data directory.
#[derive(Clone)]
struct App;

impl Host for App {
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
        // Whatever the app already runs its async work on. The task ends
        // the download by itself; nothing waits on it.
        tokio::runtime::Handle::current().spawn(task);
    }

    fn publish(&self, progress: &Progress) {
        // The app's own channel: a bus event, a channel send, a probe.
        println!(
            "{} {:?} {} bytes",
            progress.id, progress.phase, progress.received
        );
    }

    fn data_dir(&self) -> Result<PathBuf, String> {
        Ok(std::env::temp_dir().join("my-app"))
    }
}

fn main() {
    let downloads = Downloads::new();
    let request = DownloadRequest {
        id: "words".into(),
        // Several links to the SAME file, preferred first: a client whose
        // network blocks one host costs one round trip, not a failure.
        urls: vec![
            "https://example.com/words.parquet".into(),
            "https://mirror.example.org/words.parquet".into(),
        ],
        directory: Some("data".into()),
        file_name: "words.parquet".into(),
    };
    // Every snapshot, the terminal one included, reaches this hook: the
    // `done` snapshot carries the finished file's absolute path.
    let hook: ProgressHook = Arc::new(|progress: &Progress| {
        if let Some(path) = &progress.path {
            println!("words are ready at {path}");
        }
    });
    downloads.start(&App, request, Some(hook)).expect("id is free");
}
```

## The pieces

- `DownloadRequest { id, urls, directory, file_name }` — `id` keys every
  later call, event and hook, and one id runs one download. `urls` are
  mirrors of one file. `directory` is relative to the host's data directory
  and `file_name` is a bare name; both are refused if they would leave it.
  A request with no links, or an id that is not finished, is refused by
  `start`.
- `Downloads` — the registry. `start(host, request, hook)`,
  `pause(id)`, `resume(host, id)`, `cancel(host, id)`, `remove(id)`,
  `status(id)`, `list()`.
- `Progress { id, phase, received, total, message, path }` — the wire
  snapshot, one JSON object per event, with `percent()` for a bar.
  `phase` is `downloading | paused | done | failed | cancelled`; the last
  three are terminal, and `Phase::terminal()` says so.
- `Host` — `spawn`, `publish`, `data_dir`. The crate never owns a thread
  pool, an event bus or a path.
- `ProgressHook` — optional, and called on every snapshot. A feature that
  would rather be called than subscribe passes one; the terminal snapshot is
  guaranteed to reach it, which is how a caller learns where the file
  landed.

## What it survives

- **A blocked or dead mirror.** Attempt `n` walks the link list by round, so
  every mirror gets a try before any mirror gets a second. The budget is two
  rounds altogether; the retries between them back off 500 ms, 1 s, 2 s,
  4 s, 8 s.
- **A broken connection.** Every retry resumes from the bytes already on
  disk, so a transfer that dies at 90 % costs 10 %.
- **A wrong file on disk.** A partial carries the server's validator (`ETag`,
  else `Last-Modified`) in a one-line sidecar; a resume sends it back as
  `If-Range`, and a server that answers `200` instead of `206` means the
  partial is from a different resource — it is replaced from zero. A partial
  with no validator at all is deleted, never spliced.
- **A stale range.** A server that answers `416` while claiming the partial
  already covers the whole file means the download is complete; any other
  `416` drops the partial and restarts without spending a retry.
- **A stalled connection.** No byte for 30 s is a dead socket: the partial is
  flushed, and the next attempt resumes from it.
- **A pause mid-stream.** `pause` is read at the next chunk — and during a
  backoff wait, so it lands immediately on a stalled transfer too. The phase
  becomes `paused` with the byte count it stopped at, and `resume` continues
  from there.
- **Cancelling a paused download.** There is no stream left to read a flag,
  so `cancel` settles that snapshot itself.
- **Removing a live download.** The record is flagged and dropped; the stream
  closes its own handle and the partial goes with it. A finished file stays:
  the feature owns what it asked for.
- **A restart of the app.** The partial and its validator are on disk, so a
  later `start` with the same request continues where the last run stopped.

## What the bytes look like on disk

The finished file is at `<data_dir>/<directory>/<file_name>`. In transit the
same path carries `.part` (the bytes) and `.part.meta` (one line: the
validator a resume proves those bytes with). Both live beside the
destination, so a download interrupted by a crash or a kill -9 is as
resumable as one that stopped politely. A finished download leaves neither:
`done` means the partial was renamed onto the destination.

## What it does not do

- No checksums: integrity rests on the server's validator plus the declared
  size, when one is sent.
- No auth headers, cookies or proxies: a link is a link.
- No parallel connections or segmented ranges: one stream per attempt.
- No scheduler across app runs: a partial waits on disk until the feature asks
  for that id again.
