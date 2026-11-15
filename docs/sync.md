# Sync: design

> **Status:** optional future feature; database groundwork is in place · **Last updated:** 2026-11-15

mareader has no sync. mareader2 will sync one library across machines
through several backends. Sync itself is optional future work, so the library
model was built ready for it from the start: retrofitting ids, clocks and
tombstones later would mean migrating every user's library.

## What sync is for

1. **One library on every machine:** books, shelves, positions, marks
   and portable settings.
2. **Bring your own storage.** A synced folder (Dropbox, iCloud Drive,
   OneDrive's folder, Syncthing), WebDAV, S3-compatible storage, Google
   Drive and OneDrive's API.
3. **Offline first.** Every machine is complete on its own. Sync merges
   whenever a backend is reachable; nothing waits on the network.
4. **Never lose work.** Concurrent edits on two machines both survive when
   they touch different fields; a removal on one machine never deletes a
   user's own file anywhere.

## Library groundwork

These invariants are already built into the library schema so sync can land as
a transport rather than a migration.

- **Ids nobody else mints.** An original book's id derives from its first
  content id, so two machines that import the same file agree on it
  without talking. Twins, shelves, memberships and marks take random
  128-bit ids. No counters.
- **Synced and local facts kept apart.** Synced: content id, title,
  author, format, added, opened, position, marks, shelves, memberships,
  `twin_of`, `design`. Local to a machine: locations and `present`, size and mtime
  stamps, sources and their `hidden` sets, window and pane state. Sources
  are folders on one disk, so they never sync.
- **A stamp on every synced field.** A hybrid logical clock (below), so a
  rename on one machine and a shelf move on another both land.
- **Removal is a tombstone.** A removed book or shelf keeps its record
  with a removal stamp, and the UI filters it out. Records never vanish.
- **Manual order by key, not index.** A shelf's order is a sort key per
  membership (a string between its neighbours'), so two machines that
  reorder different books merge without a conflict.
- **Arranged levels too.** Books (on the top level) and shelves
  (beside their siblings) carry a stamped key of their own, merged per
  field like a membership's.
- **A device id**, random, made on first launch and kept in the data
  directory.

## Clock: hybrid logical time

A stamp is `(milliseconds, counter, device)`, packed as fixed-width hex so
comparing the strings compares the times. Making a stamp takes the larger
of the wall clock and the last stamp seen, local or remote; a tie bumps
the counter. Ties between devices fall to the device id, so every machine
picks the same winner.

**Why not the wall clock.** A machine whose clock runs an hour fast (a
wrong time zone hand-corrected by the user is common) would win every
write for that hour and lock its peers out. With hybrid stamps a peer that
sees the fast stamp moves its own clock past it, so its next edit still
wins. Any "last synced" label clamps future times to now.

## Merge rules

Merging is a pure function, `merge(local, remote) -> merged`, in the
library crate, with property tests that it is commutative, associative
and idempotent. Because every machine holds full state and re-merges, a
blind upload of the merged state converges even when a write is lost.

| Record | Rule |
|---|---|
| Book fields (title, author, opened, `twin_of`, `design`) | Last writer wins, per field |
| Position | Last writer wins; an open book shows "Read to p. 90 on another device" with Jump, and never moves the page by itself |
| Shelf (name, parent) | Per field; a move that would make a cycle is dropped, keeping the older parent |
| Membership | A record per (shelf, book): alive flag and sort key, each last writer wins |
| Mark | A set by id; fields per stamp (the saved answer, `gloss`, is one); tombstoned |
| Removal | `removed` and `restored` stamps; alive while `restored > removed`. Field edits never revive a record; only restore does |
| Settings | Per section, last writer wins; only the portable sections sync |

**Same content, two originals.** Ids derived from content make this rare,
but an edited text file imported on two machines can still give two
originals of one content. The merge folds them: the lower id survives,
the other becomes a tombstone that names it, and memberships and marks
move across.

**A newer version of a book** (a re-downloaded or re-exported file) is new
content. Importing it with "Replace" keeps the book's id, shelves and
marks and tombstones the old content, so peers stop showing a card with
no file behind it. One book, last import wins; versions never pile up.

## Backends

One small blocking trait, run on a sync worker thread. The UI never waits
on it; results arrive as `Command`s, as the session's do. No async
runtime: the HTTP client is blocking and the worker owns it.

```text
trait Remote {
    read(path) -> Option<bytes>          // None: not there
    head(path) -> Option<{size, etag}>
    list(dir)  -> Vec<Entry>
    write(path, bytes, if_match: Option<etag>) -> etag
    upload(path, local file) / download(path, local file)   // streamed
    delete(path)
}
errors: Auth | NotFound | Network | Conflict | Quota | Other
```

Every backend passes one conformance suite, run against a fake and a local
folder in CI. Known quirks the adapters absorb: S3 has no directories;
WebDAV wants each parent created top-down; OneDrive creates parents
itself; Google Drive allows two files of one name in a folder, so its
adapter caches ids and picks the oldest. Writes are conditional (`If-Match`)
where the backend supports it; elsewhere a lost race heals next round
because merging is state-based.

Credentials live in the system keychain, never in the settings file.

## Wire layout (file backends)

```text
<root>/mareader/
  library.json                    books, shelves, memberships (synced fields)
  state/<book-id>.json            position and marks of one book
  files/<content-id>/<title>.<ext>   the file, when file sync is on
  files/<content-id>/cover.jpg
```

Each JSON file is an envelope: `{ schema, device, records }`. The layout
is frozen once shipped; later versions only add paths, which older
clients ignore. Content-id directories mean a rename never renames a
remote file. Text files sync as their source, which converts the same
way on every machine (`docs/epub.md` § Text files as EPUBs).

## A sync round

1. Pull `library.json` when its etag changed; merge; apply.
2. Push the merged state when it differs from the remote.
3. Pull and push `state/` files of books changed on either side: a local
   dirty set plus the listing's etags.
4. Files: upload store copies the remote lacks; a book only on the remote
   shows as a cloud card with its cover and downloads when opened.

**Changes made during a round are kept.** The round snapshots the dirty
set at its start; anything edited after stays dirty for the next round.
Dropping in-flight changes is the classic way a new import never reaches
the other machines.

## Removing

- Removing a book tombstones it everywhere; the remote file stays.
- "Remove everywhere" also deletes the remote file. It is a separate,
  explicit choice with its own stamp.
- Nothing ever deletes a file in a user's source folder, on any machine.
  Only store copies the app made are its to delete (`docs/library.md`).
- Tombstones are kept for good. They are small, and dropping them lets a
  machine that was offline for months bring removed books back.

## Why SQLite (owner, 2026-10-15)

The owner reversed the plain-files decision: the library is one bundled
SQLite file, `library.sqlite` (`docs/library.md` § Storage on SQLite). The record
model above is unchanged; only where records rest moved.

- **Records** (books, shelves, memberships) are rows of `{kind, id, body,
  seq}`. The body is the record's JSON, every field stamped as above. Rows
  are never deleted; a removal is a tombstone in the body.
- **Local rows** (locations, folders) live in their own table and never
  leave the machine.
- **The change cursor.** Each save gets the next `seq` and stamps the rows
  it wrote. A sync remembers the last `seq` it pushed and asks the store
  for rows past it, so a round never diffs the whole library.
- **Merge** is `Merge` in `crates/library`: per field last-writer-wins,
  tombstones only grow. Property tests check it commutes, associates and
  is idempotent, which is what lets backends race safely.
- **Positions** are records too (kind `state`, owner's choice for sync,
  2026-10-15): one cursor finds every change, and a save is one
  transaction. A file backend may still lay them out as
  `state/<book-id>.json` on the remote.
- **Migrations** are dated, append-only SQL steps keyed by `user_version`.
  A newer file is read but never written.

The file layout above is the remote side and is unchanged: a file backend
writes records from the store into those envelopes. No backend exists yet.

## Absorbing remote rows (2026-10-16)

A review of the model against the design found four places where two
machines could end up apart or a newer build's work could be lost. They
are closed before any backend exists, so none of them becomes a migration.

- **`Library::absorb(rows, now)`** is the receiving half every backend
  will call. It merges synced rows (books, shelves, memberships, positions)
  with `Merge`, moves the clock past every stamp it saw, skips local kinds
  and unreadable rows, then mends what merging can leave, in this order:
  one content as two originals, loops of shelves, sibling shelves of one
  name. Merged and mended records are marked changed, so the next save
  writes them and the next round sends them on.
- **Mends are writes every machine makes alike.** Each picks its target
  from the merged state alone, so two machines that mend the same thing
  write the same values; their stamps differ and the later one wins
  without a visible change.
- **Fields from a newer build.** Every synced record keeps the fields this
  build doesn't know (`Extra`) and writes them back unchanged. Merging
  takes, per field, the value with the later `t`, then the larger text, so
  unknown fields merge as known ones do. This holds only if later versions
  follow the schema rule: **a new field is a top-level, stamped `{v, t}`
  field of its record; a field never changes meaning; a value inside a
  field never grows.** Something with a new meaning gets a new name.
  New kinds go in new paths (§ Wire layout), which older clients ignore.
- **`twin_of` is stamped.** Promoting a twin when its original is removed
  rewrites `twin_of`, so it is last writer wins like the other book fields;
  the old larger-id-wins rule brought a promoted twin back under a removed
  book. A bare value saved before the stamp reads as never written.
- **Loops of shelves.** Each shelf keeps `from`, where it was before its
  last move, carried with whichever move wins a merge. A loop is broken at
  the shelf whose move is newest: it goes back to `from` while that shelf
  lives, else to the top level, and to the top level if the same shelf is
  picked again.
- **Two shelves of one name** beside each other (case folded): the one
  named first keeps the name, the others take the first free `Name 2`,
  `Name 3`, as removing a shelf already does.
- **One content as two originals** (§ Merge rules): the lower id keeps the
  book. It takes the other's title, author and last open by stamp, its
  files here, its shelves and its position; the other becomes a removed
  twin of it, and twins of the other follow.

**Left for the sync transport.** Settings carry no stamps yet; a book's
synced record has no file size for downloads; rows merged in get a new
`seq`, so a round sends them back once (harmless, merging is idempotent);
encryption would seal each `v` and leave `t` readable; marks already follow
the record pattern and future reading stats can reuse it.

## Revision history

| Date | Summary |
|---|---|
| 2026-10-15 | First design: library groundwork, clock, merge rules, backends, layout |
| 2026-10-15 | SQLite store replaces plain files (owner): records, local rows, change cursor, `Merge` |
| 2026-10-16 | Absorbing remote rows: `absorb`, mends, fields from newer builds, stamped `twin_of` |
