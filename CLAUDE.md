@AGENTS.md

# mareader2: agent quick start

Binding rules live in [`AGENTS.md`](AGENTS.md) (which always wins). Repository
layout, build commands, and the screenshot look loop live in
[`docs/development.md`](docs/development.md).

## Session start

1. Check verification mode: `python3 tools/ci.py mode`, then follow
   `AGENTS.md` § Session start (`local`, `cache` or `ci`). In cache mode, run
   `python3 tools/ci.py cache status` and `source /home/user/.cache/env.sh`
   before any cargo command.
2. Read [`ROADMAP.md`](ROADMAP.md) for current status and open work, and check
   [`docs/architecture.md`](docs/architecture.md) and
   [`docs/design.md`](docs/design.md) before modifying engine boundaries or UI.
3. For parity work, run `cargo xtask parity --list planned` (or `partial`) and
   follow [`docs/porting.md`](docs/porting.md) § How to rebuild one feature.

Don't stop to ask unless a decision is on the owner list below.

## Owner decisions (ask, don't guess)

- Allowing any new `unsafe` (the complete exception list is in `AGENTS.md`).
- Adding a dependency that is heavy (over ~50 transitive crates), copyleft, or
  replaces one named in an ADR (parley for EPUB is settled).
- Dropping or changing a behaviour mareader users rely on. (Improving the look
  or design for the same outcome needs no permission.)
- Adding an asset whose licence is not on the `AGENTS.md` allowlist.
- Dropping a parity feature.
